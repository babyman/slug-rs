#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget can leave ESP HAL peripherals in an invalid state."
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;

mod flash_program;

use esp_backtrace as _;
use esp_hal::{clock::CpuClock, main};
use slug_vm::{
    NativeArity, NativeCall, NativeModule, NativeOwnedValue, NativeStatus, Value, Vm,
};

// The ESP-IDF bootloader requires this application descriptor.
esp_bootloader_esp_idf::esp_app_desc!();

fn uart_println(call: &mut NativeCall<'_>) -> NativeStatus {
    let value = match call.argument(0) {
        Ok(value) => value,
        Err(error) => return call.raise(error),
    };
    esp_println::println!("{}", value.to_display_string());
    call.return_value(NativeOwnedValue::nil())
}

/// Runs a direct bytecode program against the minimal UART capability.
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
    let _peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73_744);

    let (program, entry) = match flash_program::load_entry_program() {
        Ok(program) => program,
        Err(error) => panic!("failed to load flash program: {error}"),
    };
    let module = NativeModule::new("slug.uart", ()).expect("UART module name is valid");
    let mut vm = Vm::new();
    vm.define_native(
        module
            .function("println", NativeArity::Exact(1), uart_println)
            .expect("UART println descriptor is valid"),
    )
    .expect("UART println is installed once");
    loop {
        match vm.run(&program, entry) {
            Ok(Value::Nil) => esp_println::println!("Slug VM completed"),
            Ok(value) => esp_println::println!("Slug VM unexpected result: {value:?}"),
            Err(error) => esp_println::println!("Slug VM runtime error: {error}"),
        }
        for _ in 0..30_000_000 {
            core::hint::spin_loop();
        }
    }
}
