//! Neutral types for QML: flat items (serde camelCase → QVariantMap in the shell), list changes,
//! playback context, string ids. Neither Spotify nor Qt is here — the mapping from the Web API lives in sf-api.
//! The serialization keys are a binding contract with QML.
#![allow(clippy::new_without_default)]

pub mod context;
pub mod ids;
pub mod items;
pub mod list;
pub mod trackfilter;

pub use context::{PlayContext, contexts};
pub use ids::{AccountId, PlaylistId};
pub use items::*;
pub use list::ListChange;
