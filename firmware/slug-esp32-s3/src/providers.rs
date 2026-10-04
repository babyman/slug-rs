//! Firmware-native providers for the frozen GPIO blink image.
//!
//! These callbacks own the selected output and blocking delay facility for the
//! VM lifetime. They intentionally expose only the board-profile `led` alias;
//! source never selects raw pins or electrical configuration.

use alloc::format;
use core::cell::RefCell;

use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};
use slug_vm::{
    NativeArity, NativeCall, NativeDescriptorError, NativeError, NativeModule, NativeOwnedValue,
    NativeStatus, NativeValueKind, NativeValueRef, Vm,
};

use crate::capabilities::{LED_ACTIVE_HIGH, MAXIMUM_SLEEP_MS};

struct GpioState {
    led: RefCell<Output<'static>>,
}

/// Installs the selected GPIO and blocking-delay capabilities into `vm`.
///
/// `led` must already be configured to its known inactive electrical level.
/// The VM retains it as native module state until firmware shutdown.
///
/// # Errors
///
/// Returns a checked descriptor error if a private frozen-image binding was
/// already installed.
pub fn install(vm: &mut Vm, led: Output<'static>) -> Result<(), NativeDescriptorError> {
    let gpio = NativeModule::new(
        "slug.gpio",
        GpioState {
            led: RefCell::new(led),
        },
    )?;
    let time = NativeModule::new("slug.time", Delay::new())?;
    vm.define_native(gpio.function("gpio.write", NativeArity::Exact(2), gpio_write)?)?;
    vm.define_native(time.function("time.sleepMs", NativeArity::Exact(1), sleep_ms)?)?;
    Ok(())
}

/// Creates the board-profile output at the inactive level.
#[must_use]
pub fn output(pin: esp_hal::peripherals::GPIO2<'static>) -> Output<'static> {
    Output::new(pin, inactive_level(), OutputConfig::default())
}

fn gpio_write(call: &mut NativeCall<'_>) -> NativeStatus {
    let alias = match call.argument(0).and_then(NativeValueRef::as_str) {
        Ok(alias) => alias,
        Err(error) => return call.raise(error),
    };
    let level = match call.argument(1).and_then(NativeValueRef::as_bool) {
        Ok(level) => level,
        Err(error) => return call.raise(error),
    };
    if alias != "led" {
        return call.raise(NativeError::new(
            "gpio.alias",
            format!("GPIO output alias `{alias}` is unavailable"),
        ));
    }
    let Some(state) = call.state::<GpioState>() else {
        return call.report_contract_violation("GPIO callback has incompatible module state");
    };
    state.led.borrow_mut().set_level(level_for(level));
    esp_println::println!("Slug GPIO led -> {}", if level { "high" } else { "low" });
    call.return_value(NativeOwnedValue::nil())
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

const fn inactive_level() -> Level {
    level_for(false)
}

const fn level_for(level: bool) -> Level {
    match level == LED_ACTIVE_HIGH {
        true => Level::High,
        false => Level::Low,
    }
}
