//! The internal "queue-state" modules: the player actor, the queue model (queue.md), merging Local/Remote, holds.
//! Create new top-layer sf-player files only here; declare them in this mod.rs. The queue tests live here too.
pub(crate) mod actor;
pub mod backend;
/// Spotify Connect through the librespot session (the connect-state cluster, commands to other devices)
pub mod connect;
pub mod core;
pub(crate) mod diff;
pub(crate) mod host;
pub mod map;
pub mod session;
#[cfg(test)]
mod tests;
