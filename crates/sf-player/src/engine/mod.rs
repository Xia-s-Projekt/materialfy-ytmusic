//! Bottom layer of sf-player (provider "engine"): librespot Engine, custom sink on fastframe-audio, EQ/limiter,
//! level tap, reads through the session, zeroconf, cover to file. Outward — through PlayerHandle (upper layer)
//! and direct calls to session_reads/zeroconf/sink::output_devices from sf-services.
pub mod cover;
pub mod eq;
pub mod fx;
pub mod limiter;
/// "New releases" through the session: "Release Radar", subscriptions, artist releases
pub mod new_releases;
pub mod player;
pub mod resample;
/// account collection through the session: liked tracks, albums, subscriptions (collection v2)
pub mod session_collection;
/// playlists through the session: list from rootlist, edits (playlist4 changes), creation, subscription, cover
pub mod session_playlists;
/// podcasts through the session: shows and episodes (metadata), saved shows and "Your episodes"
pub mod session_podcasts;
pub mod session_reads;
pub mod sink;
pub mod stretch;
pub mod vis;
pub mod zeroconf;

pub use player::{
    Engine, EngineConfig, EngineEvent, Interrupted, LoadSpec, LocalState, LocalTrack, Notify, Playback,
    PlaybackResume, PlayerCommand, RepeatMode, Rootlist, RootlistEntry,
};
