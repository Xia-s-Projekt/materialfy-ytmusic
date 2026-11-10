//! Internal "services-core" modules (Liked Songs cache, playlist cache, paged lists, history store…).
//! Create part 1 sf-services files only here; declare them in this mod.rs.

/// album and track through the GraphQL gateway (getAlbum, getTrack): play counts, label, copyright holders, date
pub mod album_overview;
/// artist through the GraphQL gateway (queryArtistOverview): followers, listeners, similar artists, biography
pub mod artist_overview;
/// collection through the session: "Liked Songs", albums, follows, save and check
pub mod collection_session;
/// atomic file writing
pub mod fsio;
/// local listening history (ported from history.rs)
pub mod journal;
/// the account's home shelves on disk: home is visible while the session comes up
pub mod home_cache;
/// home, "Made for you", top, recent and labels by URI through the session (GraphQL and spclient), no Web API
pub mod home_feed;
/// a registry of known tracks and "records" (ISRC)
pub mod known;
/// the account's playlist tree on disk: the library is visible while the session comes up
pub mod library_cache;
/// "Liked Songs": a disk cache and optimistic intents (ported from liked.rs)
pub mod liked;
/// a paged list (PagedList ported from model.rs)
pub mod paged;
/// search through the Spotify clients' GraphQL gateway (pathfinder) with a session token
pub mod pathfinder;
/// a request to pathfinder by saved-query hash; hashes from the web player bundle, cache on disk
pub mod persisted;
/// the account profile through a librespot session (name, avatar, account type) without the Web API
pub mod session_profile;
/// a cache of playlist rows per account
pub mod playlist_cache;
/// playlists through the session: the library list from rootlist and every edit (the Web API is a fallback path)
pub mod playlist_session;
/// podcasts through the session: shows, episodes, saved; resume points — from the GraphQL gateway
pub mod podcast_session;
/// Web API / session routing and error texts
pub mod route;
/// labels, plural forms, list diffs
pub mod text;
