#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget can leave ESP HAL peripherals in an invalid state."
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;

#[path = "../capabilities.rs"]
mod capabilities;
mod flash_program;
mod providers;

use alloc::rc::Rc;
use esp_backtrace as _;
use esp_hal::{clock::CpuClock, main, system::software_reset};
use slug_vm::{EmptyVmConfiguration, Value, cslug::ArtifactHost};

use crate::capabilities::SAFE_OUTPUT_PINS;

// The ESP-IDF bootloader requires this application descriptor.
esp_bootloader_esp_idf::esp_app_desc!();

/// Runs the closed descriptor-selected artifact against firmware-owned GPIO and delay providers.
///
/// The bounded allocator serves all VM allocations. Source parsing and desktop
/// module resolution remain intentionally outside this proof firmware.
#[allow(
    clippy::large_stack_frames,
    reason = "the firmware entrypoint owns the hardware and allocator setup."
)]
#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    esp_println::println!(
        "Slug static host: {} PinPulse outputs; running the light walk",
        SAFE_OUTPUT_PINS.len()
    );

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73_744);

    let artifact = flash_program::load_target_artifact()
        .unwrap_or_else(|error| reset_after_failure("failed to decode flash artifact", error));
    let host = ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration))
        .unwrap_or_else(|error| reset_after_failure("flash artifact graph is invalid", error));
    providers::register(&host, providers::safe_output_pins(peripherals))
        .unwrap_or_else(|error| reset_after_failure("provider registration failed", error));
    match host.run() {
        Ok(Value::Nil) => reset_after_message("Slug VM completed unexpectedly"),
        Ok(value) => reset_after_message(&alloc::format!("Slug VM unexpected result: {value:?}")),
        Err(error) => reset_after_failure("Slug VM runtime error", error),
    }
}

/// Emits a startup or runtime failure before resetting instead of busy-spinning.
fn reset_after_failure(error_prefix: &str, error: impl core::fmt::Display) -> ! {
    esp_println::println!("{error_prefix}: {error}; resetting");
    software_reset()
}

/// Emits an unexpected VM completion before resetting the proof firmware.
fn reset_after_message(message: &str) -> ! {
    esp_println::println!("{message}; resetting");
    software_reset()
}
