// Board profile selected for the Lonely Binary S3 N16R8 with PinPulse Shield.

/// Expands the board's physical output policy as `(pin number, HAL field)` pairs.
///
/// The single list drives both the public allow-list and the ownership transfer
/// from `esp_hal::Peripherals`; adding a permitted pin therefore cannot update
/// one representation without the other.
macro_rules! with_safe_output_pins {
    ($consumer:ident) => {
        $consumer! {
            (4, GPIO4), (5, GPIO5), (6, GPIO6), (7, GPIO7),
            (15, GPIO15), (16, GPIO16), (17, GPIO17), (18, GPIO18),
            (8, GPIO8), (3, GPIO3), (46, GPIO46), (9, GPIO9),
            (10, GPIO10), (11, GPIO11), (12, GPIO12), (13, GPIO13),
            (14, GPIO14), (1, GPIO1), (2, GPIO2), (42, GPIO42),
            (41, GPIO41), (40, GPIO40), (39, GPIO39), (38, GPIO38),
            (0, GPIO0), (45, GPIO45), (48, GPIO48), (47, GPIO47),
            (21, GPIO21),
        }
    };
}

pub(crate) use with_safe_output_pins;

/// Largest synchronous delay accepted by the firmware provider.
pub const MAXIMUM_SLEEP_MS: u32 = 1_000;
/// PinPulse Shield's verified, broken-out push-pull output pins.
///
/// This excludes only the serial (43, 44), native USB (19, 20), and PSRAM
/// (35--37) pins. The light-walk program has been verified on this shield with
/// the boot, strapping, and RGB-connected pins driven as ordinary outputs.
macro_rules! pin_numbers {
    ($(($pin:literal, $field:ident)),+ $(,)?) => {
        &[$($pin),+]
    };
}

pub const SAFE_OUTPUT_PINS: &[u8] = with_safe_output_pins!(pin_numbers);
