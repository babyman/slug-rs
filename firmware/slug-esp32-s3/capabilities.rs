// Build-selected modules and logical outputs for the Lonely Binary S3 N16R8
// with the PinPulse Shield. Its GPIO 2 indicator is lit at a high level.
const BLINK_CAPABILITIES: slug_frontend::FrozenGpioCapabilities<'static> =
    slug_frontend::FrozenGpioCapabilities {
        modules: &["slug.gpio", "slug.time"],
        output_aliases: &["led"],
        maximum_sleep_ms: 1_000,
    };
