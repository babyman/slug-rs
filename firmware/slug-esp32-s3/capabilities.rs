// Board profile selected for the Lonely Binary S3 N16R8 with PinPulse Shield.

/// Largest synchronous delay accepted by the firmware provider.
pub const MAXIMUM_SLEEP_MS: u32 = 1_000;
/// Documented safe, broken-out push-pull output pins for this board.
///
/// This excludes PSRAM (35--37), strapping pins (0, 3, 45, 46), USB (19, 20),
/// UART0 (43, 44), and the onboard RGB LED (48).
pub const SAFE_OUTPUT_PINS: &[u8] = &[
    1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 21, 38, 39, 40, 41, 42, 47,
];
