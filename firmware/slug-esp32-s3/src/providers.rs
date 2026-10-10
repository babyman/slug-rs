//! Firmware-native providers for the closed descriptor-selected artifact.
//!
//! These callbacks own the selected outputs and blocking delay facility for the
//! VM lifetime. A program may select only a physical GPIO listed by the board
//! profile; the provider configures it lazily on its first write.

use alloc::{format, string::{String, ToString}, vec, vec::Vec};
use core::cell::RefCell;

use esp_hal::{
    delay::Delay,
    gpio::{AnyPin, Level, Output, OutputConfig, Pin},
    peripherals::Peripherals,
};
use slug_vm::{
    NativeArity, NativeCall, NativeError, NativeModule, NativeOwnedValue, NativeStatus,
    NativeValueKind, NativeValueRef,
    cslug::{ArtifactHost, StaticProvider, StaticProviderError},
};

use crate::capabilities::MAXIMUM_SLEEP_MS;

struct GpioState {
    available: RefCell<Vec<(u8, AnyPin<'static>)>>,
    outputs: RefCell<Vec<(u8, Output<'static>)>>,
}

/// Registers the selected GPIO and blocking-delay capabilities with `host`.
///
/// The VM retains claimed outputs as native module state until firmware
/// shutdown; allowed but unused pins remain unconfigured.
///
/// # Errors
///
/// Returns a checked error when the provider set does not exactly match the
/// firmware artifact's foreign contract.
pub fn register(
    host: &ArtifactHost,
    output_pins: Vec<(u8, AnyPin<'static>)>,
) -> Result<(), StaticProviderError> {
    let gpio = NativeModule::new(
        "slug.gpio",
        GpioState {
            available: RefCell::new(output_pins),
            outputs: RefCell::new(Vec::new()),
        },
    )
    .map_err(|error| StaticProviderError::Descriptor(error.to_string()))?;
    let time = NativeModule::new("slug.time", Delay::new())
        .map_err(|error| StaticProviderError::Descriptor(error.to_string()))?;
    let builtins = NativeModule::new("slug.builtin", ())
        .map_err(|error| StaticProviderError::Descriptor(error.to_string()))?;
    let functions = vec![
        gpio.function("write", NativeArity::Exact(2), gpio_write),
        time.function("sleepMs", NativeArity::Exact(1), sleep_ms),
        builtins.function("print", NativeArity::Variadic { minimum: 0 }, print),
        builtins.function("println", NativeArity::Variadic { minimum: 0 }, println),
        builtins.function("len", NativeArity::Exact(1), native_len),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| StaticProviderError::Descriptor(error.to_string()))?;
    host.install_static_providers([StaticProvider::new(functions)])
}

/// Returns each physical GPIO output this board profile permits Slug to claim.
///
/// The explicit list excludes only serial, native USB, and PSRAM pins. It
/// matches the PinPulse Shield light-walk wiring order.
#[must_use]
pub fn safe_output_pins(peripherals: Peripherals) -> Vec<(u8, AnyPin<'static>)> {
    macro_rules! collect_output_pins {
        ($(($pin:literal, $field:ident)),+ $(,)?) => {
            vec![$(($pin, peripherals.$field.degrade())),+]
        };
    }

    crate::capabilities::with_safe_output_pins!(collect_output_pins)
}

fn gpio_write(call: &mut NativeCall<'_>) -> NativeStatus {
    let pin = match call.argument(0).and_then(gpio_pin_from_value) {
        Ok(pin) => pin,
        Err(error) => return call.raise(error),
    };
    let level = match call.argument(1).and_then(NativeValueRef::as_bool) {
        Ok(level) => level,
        Err(error) => return call.raise(error),
    };
    let Some(state) = call.state::<GpioState>() else {
        return call.report_contract_violation("GPIO callback has incompatible module state");
    };
    let write_result = (|| -> Result<(), NativeError> {
        let mut outputs = state.outputs.borrow_mut();
        if let Some((_, output)) = outputs
            .iter_mut()
            .find(|(output_pin, _)| *output_pin == pin)
        {
            output.set_level(level_for(level));
            return Ok(());
        }
        let mut available = state.available.borrow_mut();
        let index = available
            .iter()
            .position(|(output_pin, _)| *output_pin == pin)
            .ok_or_else(|| {
                NativeError::new(
                    "gpio.pin",
                    format!("GPIO output pin {pin} is unavailable on this board"),
                )
            })?;
        let (_, output_pin) = available.swap_remove(index);
        drop(available);
        let mut output = Output::new(output_pin, Level::Low, OutputConfig::default());
        output.set_level(level_for(level));
        outputs.push((pin, output));
        Ok(())
    })();
    if let Err(error) = write_result {
        return call.raise(error);
    }
    esp_println::println!("Slug GPIO {pin} -> {}", if level { "high" } else { "low" });
    call.return_value(NativeOwnedValue::nil())
}

fn gpio_pin_from_value(value: NativeValueRef<'_>) -> Result<u8, NativeError> {
    match value.kind() {
        NativeValueKind::Int => u8::try_from(value.as_i64()?).map_err(|_| invalid_gpio_pin()),
        NativeValueKind::Float => {
            let value = value.as_f64()?;
            if !value.is_finite() || value < 0.0 || value > f64::from(u8::MAX) {
                return Err(invalid_gpio_pin());
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let pin = value as u8;
            if f64::from(pin) != value {
                return Err(invalid_gpio_pin());
            }
            Ok(pin)
        }
        _ => Err(invalid_gpio_pin()),
    }
}

fn invalid_gpio_pin() -> NativeError {
    NativeError::new("gpio.pin", "GPIO pin must be a whole number from 0 to 255")
}

fn print(call: &mut NativeCall<'_>) -> NativeStatus {
    write_output(call, false)
}

fn println(call: &mut NativeCall<'_>) -> NativeStatus {
    write_output(call, true)
}

fn write_output(call: &mut NativeCall<'_>, newline: bool) -> NativeStatus {
    let mut output = String::new();
    for index in 0..call.argument_count() {
        if index > 0 {
            output.push(' ');
        }
        output.push_str(
            &call
                .argument(index)
                .expect("index comes from the argument count")
                .to_display_string(),
        );
    }
    if newline {
        esp_println::println!("{output}");
    } else {
        esp_println::print!("{output}");
    }
    call.return_value(NativeOwnedValue::nil())
}

fn native_len(call: &mut NativeCall<'_>) -> NativeStatus {
    let value = match call.argument(0) {
        Ok(value) => value,
        Err(error) => return call.raise(error),
    };
    let length = match value.kind() {
        NativeValueKind::String => match value.as_str() {
            Ok(value) => value.chars().count(),
            Err(error) => return call.raise(error),
        },
        NativeValueKind::Bytes => match value.as_bytes() {
            Ok(value) => value.len(),
            Err(error) => return call.raise(error),
        },
        NativeValueKind::List | NativeValueKind::Map => {
            value.len().expect("collection kind has a length")
        }
        kind => {
            return call.raise(NativeError::new(
                "native.type",
                format!("`len` expects str, bytes, list, or map, got {kind:?}"),
            ));
        }
    };
    let Ok(length) = i64::try_from(length) else {
        return call.raise(NativeError::new(
            "native.range",
            "`len` result exceeds the supported integer range",
        ));
    };
    call.return_value(NativeOwnedValue::integer(length))
}

fn sleep_ms(call: &mut NativeCall<'_>) -> NativeStatus {
    let milliseconds = match call.argument(0).and_then(milliseconds_from_value) {
        Ok(milliseconds) => milliseconds,
        Err(error) => return call.raise(error),
    };
    let Some(delay) = call.state::<Delay>() else {
        return call.report_contract_violation("time callback has incompatible module state");
    };
    if milliseconds > MAXIMUM_SLEEP_MS {
        return call.raise(NativeError::new(
            "time.range",
            format!("sleep duration {milliseconds}ms exceeds host maximum {MAXIMUM_SLEEP_MS}ms"),
        ));
    }
    delay.delay_millis(milliseconds);
    call.return_value(NativeOwnedValue::nil())
}

fn milliseconds_from_value(value: NativeValueRef<'_>) -> Result<u32, NativeError> {
    match value.kind() {
        NativeValueKind::Int => u32::try_from(value.as_i64()?).map_err(|_| invalid_milliseconds()),
        NativeValueKind::Float => milliseconds_from_float(value.as_f64()?),
        _ => Err(invalid_milliseconds()),
    }
}

fn milliseconds_from_float(value: f64) -> Result<u32, NativeError> {
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
        return Err(invalid_milliseconds());
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let milliseconds = value as u32;
    if f64::from(milliseconds) != value {
        return Err(invalid_milliseconds());
    }
    Ok(milliseconds)
}

fn invalid_milliseconds() -> NativeError {
    NativeError::new(
        "time.milliseconds",
        "sleep duration must be a whole number of milliseconds",
    )
}

const fn level_for(level: bool) -> Level {
    match level {
        true => Level::High,
        false => Level::Low,
    }
}
