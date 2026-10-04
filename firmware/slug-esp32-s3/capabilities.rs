// Board profile selected for the Lonely Binary S3 N16R8 with PinPulse Shield.

/// Static modules available to the frozen blink program.
#[allow(
    dead_code,
    reason = "this manifest is consumed by build.rs, not by the firmware binary"
)]
pub const BLINK_MODULES: &[&str] = &["slug.gpio", "slug.time"];
/// Logical output aliases available to the frozen blink program.
#[allow(
    dead_code,
    reason = "this manifest is consumed by build.rs, not by the firmware binary"
)]
pub const BLINK_OUTPUT_ALIASES: &[&str] = &["led"];
/// Largest synchronous delay accepted by the firmware provider.
pub const MAXIMUM_SLEEP_MS: u32 = 1_000;
/// PinPulse Shield's plain indicator LED is attached to this pin.
pub const LED_GPIO: u8 = 2;
/// The PinPulse Shield lights its indicator when GPIO 2 is high.
pub const LED_ACTIVE_HIGH: bool = true;
