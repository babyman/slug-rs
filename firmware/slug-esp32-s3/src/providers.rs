//! Firmware-native providers for the closed GPIO blink artifact.
//!
//! These callbacks own the selected output and blocking delay facility for the
//! VM lifetime. They expose the board-profile `led` and `led4` aliases;
//! source never selects raw pins or electrical configuration.

use alloc::format;
use core::cell::RefCell;

use esp_hal::{
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
};
use slug_vm::{
    NativeArity, NativeCall, NativeDescriptorError, NativeError, NativeModule, NativeOwnedValue,
    NativeStatus, NativeValueKind, NativeValueRef,
    cslug::ArtifactHost,
};

use crate::capabilities::{
    LED4_ACTIVE_HIGH, LED4_ALIAS, LED_ACTIVE_HIGH, LED_ALIAS, MAXIMUM_SLEEP_MS,
};

struct GpioState {
    led: RefCell<Output<'static>>,
    led4: RefCell<Output<'static>>,
}

/// Registers the selected GPIO and blocking-delay capabilities with `host`.
///
/// `led` must already be configured to its known inactive electrical level.
/// The VM retains it as native module state until firmware shutdown.
///
/// # Errors
///
/// Returns a checked descriptor error if an artifact-host binding was
/// already installed.
pub fn register(
    host: &ArtifactHost,
    led: Output<'static>,
    led4: Output<'static>,
) -> Result<(), NativeDescriptorError> {
    let gpio = NativeModule::new(
        "slug.gpio",
        GpioState {
            led: RefCell::new(led),
            led4: RefCell::new(led4),
        },
    )?;
    let time = NativeModule::new("slug.time", Delay::new())?;
    host.register_foreign(gpio.function("write", NativeArity::Exact(2), gpio_write)?)?;
    host.register_foreign(time.function("sleepMs", NativeArity::Exact(1), sleep_ms)?)?;
    Ok(())
}

/// Creates the board-profile output at the inactive level.
#[must_use]
pub fn output_2(pin: esp_hal::peripherals::GPIO2<'static>) -> Output<'static> {
    Output::new(pin, inactive_level(LED_ACTIVE_HIGH), OutputConfig::default())
}

/// Creates GPIO 4 at the inactive level for the second host-selected output.
#[must_use]
pub fn output_4(pin: esp_hal::peripherals::GPIO4<'static>) -> Output<'static> {
    Output::new(pin, inactive_level(LED4_ACTIVE_HIGH), OutputConfig::default())
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
    let Some(state) = call.state::<GpioState>() else {
        return call.report_contract_violation("GPIO callback has incompatible module state");
    };
    match alias {
        LED_ALIAS => state
            .led
            .borrow_mut()
            .set_level(level_for(level, LED_ACTIVE_HIGH)),
        LED4_ALIAS => state
            .led4
            .borrow_mut()
            .set_level(level_for(level, LED4_ACTIVE_HIGH)),
        _ => {
            return call.raise(NativeError::new(
                "gpio.alias",
                format!("GPIO output alias `{alias}` is unavailable"),
            ));
        }
    }
    esp_println::println!("Slug GPIO {alias} -> {}", if level { "high" } else { "low" });
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

const fn inactive_level(active_high: bool) -> Level {
    level_for(false, active_high)
}

const fn level_for(level: bool, active_high: bool) -> Level {
    match level == active_high {
        true => Level::High,
        false => Level::Low,
    }
}
