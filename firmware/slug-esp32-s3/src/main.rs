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

use esp_backtrace as _;
use esp_hal::{clock::CpuClock, main};
use slug_vm::{EmptyVmConfiguration, Value, cslug::ArtifactHost};
use alloc::rc::Rc;

use crate::capabilities::{LED4_GPIO, LED_ACTIVE_HIGH, LED_GPIO};

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
        "Slug static host: led -> GPIO {LED_GPIO} ({}), led4 -> GPIO {LED4_GPIO} (active high)",
        if LED_ACTIVE_HIGH {
            "active high"
        } else {
            "active low"
        }
    );

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73_744);

    let artifact = match flash_program::load_blink_artifact() {
        Ok(artifact) => artifact,
        Err(error) => panic!("failed to decode flash artifact: {error}"),
    };
    let host = ArtifactHost::new(artifact, Rc::new(EmptyVmConfiguration))
        .expect("flash artifact graph is valid");
    providers::register(
        &host,
        providers::output_2(peripherals.GPIO2),
        providers::output_4(peripherals.GPIO4),
    )
        .expect("GPIO and time providers are registered once");
    loop {
        match host.run() {
            Ok(Value::Nil) => esp_println::println!("Slug VM completed unexpectedly"),
            Ok(value) => esp_println::println!("Slug VM unexpected result: {value:?}"),
            Err(error) => esp_println::println!("Slug VM runtime error: {error}"),
        }
        for _ in 0..30_000_000 {
            core::hint::spin_loop();
        }
    }
}
