// Board profile selected for the Lonely Binary S3 N16R8 with PinPulse Shield.

/// Largest synchronous delay accepted by the firmware provider.
pub const MAXIMUM_SLEEP_MS: u32 = 1_000;
/// PinPulse Shield's plain indicator LED is attached to this pin.
pub const LED_GPIO: u8 = 2;
/// Second host-selected output available to the blink program.
pub const LED4_GPIO: u8 = 4;
/// Source alias for the PinPulse Shield indicator on GPIO 2.
pub const LED_ALIAS: &str = "led";
/// Source alias for the second host-selected output on GPIO 4.
pub const LED4_ALIAS: &str = "led4";
/// The PinPulse Shield lights its indicator when GPIO 2 is high.
pub const LED_ACTIVE_HIGH: bool = true;
/// GPIO 4 uses the same active-high output profile.
pub const LED4_ACTIVE_HIGH: bool = true;
