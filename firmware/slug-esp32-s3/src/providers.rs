//! Firmware-native providers for the closed GPIO blink artifact.
//!
//! These callbacks own the selected outputs and blocking delay facility for the
//! VM lifetime. A program may select only a physical GPIO listed by the board
//! profile; the provider configures it lazily on its first write.

use alloc::{format, vec, vec::Vec};
use core::cell::RefCell;

use esp_hal::{
    delay::Delay,
    gpio::{AnyPin, Level, Output, OutputConfig, Pin},
    peripherals::Peripherals,
};
use slug_vm::{
    NativeArity, NativeCall, NativeDescriptorError, NativeError, NativeModule, NativeOwnedValue,
    NativeStatus, NativeValueKind, NativeValueRef, cslug::ArtifactHost,
};

use crate::capabilities::{MAXIMUM_SLEEP_MS, SAFE_OUTPUT_PINS};

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
/// Returns a checked descriptor error if an artifact-host binding was
/// already installed.
pub fn register(
    host: &ArtifactHost,
    output_pins: Vec<(u8, AnyPin<'static>)>,
) -> Result<(), NativeDescriptorError> {
    let gpio = NativeModule::new(
        "slug.gpio",
        GpioState {
            available: RefCell::new(output_pins),
            outputs: RefCell::new(Vec::new()),
        },
    )?;
    let time = NativeModule::new("slug.time", Delay::new())?;
    host.register_foreign(gpio.function("write", NativeArity::Exact(2), gpio_write)?)?;
    host.register_foreign(time.function("sleepMs", NativeArity::Exact(1), sleep_ms)?)?;
    Ok(())
}

/// Returns each physical GPIO output this board profile permits Slug to claim.
///
/// The explicit list keeps serial, native USB, PSRAM, strapping, and RGB pins
/// out of the capability surface even though the HAL exposes their tokens.
#[must_use]
pub fn safe_output_pins(peripherals: Peripherals) -> Vec<(u8, AnyPin<'static>)> {
    debug_assert_eq!(
        SAFE_OUTPUT_PINS,
        &[
            1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 21, 38, 39, 40, 41, 42, 47
        ]
    );
    vec![
        (1, peripherals.GPIO1.degrade()),
        (2, peripherals.GPIO2.degrade()),
        (4, peripherals.GPIO4.degrade()),
        (5, peripherals.GPIO5.degrade()),
        (6, peripherals.GPIO6.degrade()),
        (7, peripherals.GPIO7.degrade()),
        (8, peripherals.GPIO8.degrade()),
        (9, peripherals.GPIO9.degrade()),
        (10, peripherals.GPIO10.degrade()),
        (11, peripherals.GPIO11.degrade()),
        (12, peripherals.GPIO12.degrade()),
        (13, peripherals.GPIO13.degrade()),
        (14, peripherals.GPIO14.degrade()),
        (15, peripherals.GPIO15.degrade()),
        (16, peripherals.GPIO16.degrade()),
        (17, peripherals.GPIO17.degrade()),
        (18, peripherals.GPIO18.degrade()),
        (21, peripherals.GPIO21.degrade()),
        (38, peripherals.GPIO38.degrade()),
        (39, peripherals.GPIO39.degrade()),
        (40, peripherals.GPIO40.degrade()),
        (41, peripherals.GPIO41.degrade()),
        (42, peripherals.GPIO42.degrade()),
        (47, peripherals.GPIO47.degrade()),
    ]
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
