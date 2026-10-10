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
use esp_hal::{clock::CpuClock, main};
use slug_vm::{EmptyVmConfiguration, Value, cslug::ArtifactHost};

use crate::capabilities::SAFE_OUTPUT_PINS;

// The ESP-IDF bootloader requires this application descriptor.
esp_bootloader_esp_idf::esp_app_desc!();

/// Runs the closed blink artifact against firmware-owned GPIO and delay providers.
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

    let artifact = match flash_program::load_blink_artifact() {
        Ok(artifact) => artifact,
        Err(error) => panic!("failed to decode flash artifact: {error}"),
    };
    let host = ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration))
        .expect("flash artifact graph is valid");
    providers::register(&host, providers::safe_output_pins(peripherals))
        .expect("GPIO and time providers are registered once");
    loop {
        match host.run() {
            Ok(Value::Nil) => esp_println::println!("Slug VM completed unexpectedly"),
            Ok(value) => esp_println::println!("Slug VM unexpected result: {value:?}"),
            Err(error) => {
                esp_println::println!("Slug VM runtime error: {error}");
                break;
            }
        }
        for _ in 0..30_000_000 {
            core::hint::spin_loop();
        }
    }
    loop {
        core::hint::spin_loop();
    }
}
