//! Legacy preset catalog.
//!
//! P6 intentionally ships with no bundled third-party providers. New databases
//! start with both applications in official-login mode; users add or import
//! providers explicitly.

use crate::provider::Provider;

pub struct Preset;

pub fn presets() -> &'static [Preset] {
    &[]
}

pub fn preset_to_provider(
    _preset: &Preset,
    _id: String,
    _sort_index: i64,
    _created_at: i64,
) -> Provider {
    unreachable!("P6 does not create built-in provider presets")
}
