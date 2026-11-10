//! YouTube Music via ytmusicapi (sigma67): Python bridge client + mapping to md3-model.
//!
//! The original app used the Spotify Web API (sf-api) + librespot (sf-player).
//! This fork replaces that stack with `ytmusicapi`, which emulates the
//! YouTube Music web client (InnerTube). All catalogue/library calls go
//! through `python/yt_bridge.py` so the exact upstream library is used.

pub mod client;
pub mod http;
pub mod innertube;
pub mod link;
pub mod map;
pub mod util;

pub use client::{YtClient, YtError};
pub use http::{Http, WebApiLimits};
/// Compat alias for old sf-api::ApiError uses.
pub type ApiError = YtError;
