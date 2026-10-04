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
use slug_vm::{Chunk, Op, Program, Value, Vm};

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

    let mut main = Chunk::new("main", 0);
    let seven = main.constant(Value::Int(7));
    let six = main.constant(Value::Int(6));
    main.emit(Op::Constant(seven))
        .emit(Op::Constant(six))
        .emit(Op::Multiply)
        .emit(Op::Return);

    let mut program = Program::new();
    let entry = program.add_chunk(main);
    let result = Vm::new().run(&program, entry);
    loop {
        match &result {
            Ok(Value::Int(value)) => esp_println::println!("Slug VM result: {value}"),
            Ok(value) => esp_println::println!("Slug VM unexpected result: {value:?}"),
            Err(error) => esp_println::println!("Slug VM runtime error: {error}"),
        }
        for _ in 0..30_000_000 {
            core::hint::spin_loop();
        }
    }
}
