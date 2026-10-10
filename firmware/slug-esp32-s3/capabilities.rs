// Board profile selected for the Lonely Binary S3 N16R8 with PinPulse Shield.

/// Largest synchronous delay accepted by the firmware provider.
pub const MAXIMUM_SLEEP_MS: u32 = 1_000;
/// PinPulse Shield's verified, broken-out push-pull output pins.
///
/// This excludes only the serial (43, 44), native USB (19, 20), and PSRAM
/// (35--37) pins. The light-walk program has been verified on this shield with
/// the boot, strapping, and RGB-connected pins driven as ordinary outputs.
pub const SAFE_OUTPUT_PINS: &[u8] = &[
    4, 5, 6, 7, 15, 16, 17, 18, 8, 3, 46, 9, 10, 11, 12, 13, 14, 1, 2, 42, 41, 40, 39, 38, 0, 45,
    48, 47, 21,
];
