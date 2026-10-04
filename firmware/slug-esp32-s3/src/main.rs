#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget can leave ESP HAL peripherals in an invalid state."
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;

use esp_backtrace as _;
use esp_hal::{clock::CpuClock, main};

// The ESP-IDF bootloader requires this application descriptor.
esp_bootloader_esp_idf::esp_app_desc!();

/// Verifies the first firmware milestone before the Slug runtime is linked.
///
/// The bounded allocator is intentionally initialized here: later Slug values,
/// source parsing, and bytecode will allocate from this same heap.
#[allow(
    clippy::large_stack_frames,
    reason = "the firmware entrypoint owns the hardware and allocator setup."
)]
#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let _peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73_744);

    esp_println::println!("Slug ESP32-S3 firmware bootstrap ready");

    loop {
        core::hint::spin_loop();
    }
}
