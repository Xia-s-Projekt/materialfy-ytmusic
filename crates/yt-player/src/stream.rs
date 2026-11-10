//! Player-side settings helpers.
//!
//! Format picking lives in `yt-api::innertube`: the InnerTube `player`
//! response carries plain `url` formats, so no cipher decoding is needed here.

/// Output device names (rodio defaults to system default; listed for Settings).
pub fn output_devices() -> Vec<String> {
    vec!["System Default".to_string()]
}
