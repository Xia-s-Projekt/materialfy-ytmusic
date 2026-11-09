# Materialfy → YouTube Music migration

Fork of [fatyzzz/materialfy](https://github.com/fatyzzz/materialfy) (Xia-s-Projekt org).

## Why ytmusicapi?

User request: adapt Materialfy from Spotify to YTMusic using
[sigma67/ytmusicapi](https://github.com/sigma67/ytmusicapi). The Python library already covers
search, home, library, playlists, albums, artists, watch playlists and `get_song` stream URLs.
A native Innertube Rust rewrite would duplicate that work, so this fork uses a Python sidecar bridge.

## Bridge

- `crates/yt-api/python/yt_bridge.py` — one-shot bridge; Rust `YtClient` spawns
  `python3 yt_bridge.py <method> <json>` and reads one JSON object from stdout.
- Methods: `search`, `get_home`, library (`get_liked_songs`, `get_library_songs`,
  `get_library_albums`, `get_library_artists`, `get_library_playlists`,
  `get_library_subscriptions`), `get_playlist`, `get_album`, `get_artist`, `get_song`,
  `get_watch_playlist`, `get_lyrics`, `get_charts`, `rate_song`, `edit_song_library`,
  `create_playlist`, `edit_playlist`, `delete_playlist`, `add_playlist_items`,
  `edit_playlist_items`, `delete_playlist_items`.
- Requires Python 3 and `pip install ytmusicapi`. Missing Python/sidecar surfaces as
  `YtError::PythonMissing`.

## Crate map

| Spotify | YouTube Music |
|---|---|
| `sf-auth` | `yt-auth` — validates browser headers-auth JSON, file store |
| `sf-api` | `yt-api` — bridge client, `YtError`, YT link parser (`music.youtube.com` / `youtube.com` / `youtu.be`), `md3-model` mapping |
| `sf-player` | `yt-player` — queue actor, `get_song` stream URL picker; audio output stubbed |
| `sf-services` | `yt-services` — service façade mirroring `sf-services` bridge API (`AuthService`, `CatalogService`, `LibraryService`, `HomeService`, `HistoryService`, `DevicesService`, `WaveService`, `LyricsService`, `ReleasesService`, `SocialService`, `UpdateService`) |

Old `sf-*` crates are kept in the workspace but `materialfy` now depends on `yt-*`.

## Auth

YouTube Music has no public OAuth client for desktop apps. `yt-auth` validates and stores
browser headers-auth JSON:

```bash
# https://music.youtube.com → DevTools → Network → copy request headers
# into ~/.ytmusic_headers_auth (or headers_auth.json)
```

## URIs

- Spotify: `spotify:track:<id>` → YouTube Music: `ytmusic:track:<id>` (same for
  `playlist`/`album`/`artist`/`show`/`search`).
- QML URI fallbacks were rewritten from `spotify:` to `ytmusic:`; `MiniPlayer` drop handler now
  accepts `ytmusic:` and `music.youtube.com` / `youtube.com/watch` links.

## Known limitations

- **Local audio playback is a stub.** `yt-player` resolves stream URLs via `get_song` and drives
  the queue state machine preserved from `sf-player`, but there is no audio sink: rodio playback
  conflicts with the fastframe/cpal 0.18/alsa stack used by the original app. Wiring a
  `rodio` `Sink` + `rodio::Decoder` (or a `cpal` output) is the next step.
- **Connect / remote devices, social (friend activity), jam** are stubs; YouTube Music has no
  equivalent APIs.
- **Update service** points at `Xia-s-Projekt/materialfy-ytmusic` releases but cannot
  auto-install yet.
- **I18n/QML strings** mostly still say "Spotify" — URI fallbacks are fixed, full string
  localization is TODO.

## Build / test

```bash
cargo check -p yt-auth -p yt-api -p yt-player -p yt-services -p materialfy
cargo test -p yt-auth -p yt-api -p yt-player
```
