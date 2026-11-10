//! Internal modules of "services-extra" (radio stations, media keys/SMTC/MPRIS, zeroconf receivers…).
//! New files of part 2 of sf-services are created only here; declare them in this mod.rs.

/// Connect device list: merging Web API, this PC and mDNS receivers
pub mod connect;
/// home shelves from Spotify's home and "For You" (through the session), fallback top from history
pub mod feed;
/// the "Now playing" panel and media keys (fastframe-now-playing)
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub mod media;
/// radio seed groups
pub mod seeds;
/// home shelf composition
pub mod shelves;
