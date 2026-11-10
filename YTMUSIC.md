# Materialfy → YouTube Music migration

Fork of [fatyzzz/materialfy](https://github.com/fatyzzz/materialfy) (Xia-s-Projekt org).

## Why ytmusicapi?

User request: adapt Materialfy from Spotify to YTMusic using
[sigma67/ytmusicapi](https://github.com/sigma67/ytmusicapi). The Python library already covers
search, home, library, playlists, albums, artists and watch playlists, so catalogue and library
calls go through a Python sidecar bridge.

Playback does **not** use ytmusicapi or yt-dlp. Its signed-in `WEB_REMIX` player response only
carries `signatureCipher` URLs, which need JS deciphering before Google will serve them. Instead
`yt-api::innertube` posts to `youtubei/v1/player` with the anonymous `ANDROID` client, which
returns plain `url` formats, and `yt-player` streams those bytes directly.

## Bridge

- `crates/yt-api/python/yt_bridge.py` — one-shot bridge; Rust `YtClient` spawns
  `python3 yt_bridge.py <method> <json>` and reads one JSON object from stdout.
- Methods: `search`, `get_home`, library (`get_liked_songs`, `get_library_songs`,
  `get_library_albums`, `get_library_artists`, `get_library_playlists`,
  `get_library_subscriptions`), `get_playlist`, `get_album`, `get_artist`, `get_song`,
  `get_watch_playlist`, `get_lyrics`, `get_charts`, `rate_song`, `edit_song_library`,
  `create_playlist`, `edit_playlist`, `delete_playlist`, `add_playlist_items`,
  `edit_playlist_items`, `delete_playlist_items`.
- Requires Python 3 only. `ytmusicapi` (+ deps) is **vendored** in
  `crates/yt-api/python/vendor/` — no `pip install` needed; `./install.sh` skips the
  `pip install --target` fallback when the vendor dir is already there. Missing
  Python/sidecar surfaces as `YtError::PythonMissing`.

## Crate map

| Spotify | YouTube Music |
|---|---|
| `sf-auth` | `yt-auth` — validates browser headers-auth JSON, file store |
| `sf-api` | `yt-api` — bridge client, `YtError`, YT link parser (`music.youtube.com` / `youtube.com` / `youtu.be`), `md3-model` mapping, InnerTube `player` (`innertube.rs`) |
| `sf-player` | `yt-player` — queue actor, InnerTube stream URL, streaming AAC decode through rodio |
| `sf-services` | `yt-services` — service façade mirroring `sf-services` bridge API (`AuthService`, `CatalogService`, `LibraryService`, `HomeService`, `HistoryService`, `DevicesService`, `WaveService`, `LyricsService`, `ReleasesService`, `SocialService`, `UpdateService`) |

Old `sf-*` crates are kept in the workspace but `materialfy` now depends on `yt-*`.

## Auth

YouTube Music has no public OAuth client for desktop apps. `yt-auth` validates and stores
browser headers-auth JSON. In the app: open **Account**, paste the JSON into the box and
tap **Save & sign in** — `Auth.saveHeaders` calls
`AuthService::save_headers` (validates via `HeadersAuth::is_valid`, saves, then
`start()` re-checks with a live API call). The file form still works:

```bash
# https://music.youtube.com → DevTools → Network → copy request headers
# into ~/.ytmusic_headers_auth (or headers_auth.json)
```

## URIs

- Spotify: `spotify:track:<id>` → YouTube Music: `ytmusic:track:<id>` (same for
  `playlist`/`album`/`artist`/`show`/`search`).
- QML URI fallbacks were rewritten from `spotify:` to `ytmusic:`; `MiniPlayer` drop handler now
  accepts `ytmusic:` and `music.youtube.com` / `youtube.com/watch` links.

## Playback

1. `yt-player` asks `yt-api` for a stream URL. `innertube.rs` POSTs to
   `youtubei/v1/player` (anonymous `ANDROID` client, `clientVersion 20.10.38`) and picks the
   best audio-only format with a plain `url`, preferring AAC-LC `audio/mp4; codecs="mp4a.40.2"`
   because symphonia/rodio has no Opus decoder.
2. `audio.rs` opens the default cpal device once, then starts a `StreamReader` that fetches
   256 KiB HTTP `Range` windows from the returned CDN URL on demand. rodio's `Decoder` reads
   and seeks through it, so the `moov` atom lookup is a small ranged request, not a full
   download.
3. A background thread decodes into a bounded PCM channel (8192 frames × 16 batches, roughly
   three seconds of audio). The rodio callback only pops samples and never touches the
   network, so a slow CDN stalls the decoder thread instead of the audio device.
4. Seek restarts the decoder at the new position via `Decoder::try_seek`; position is
   `base_ms + player.get_pos()`.

Nothing is written to disk and nothing is downloaded up front. `cargo run -p yt-api --example
dl_test -- <videoId>` resolves a URL without playing, and `cargo run -p yt-player --example
play_test -- <videoId>` plays it end to end.

## Known limitations

- **InnerTube `ANDROID` client.** Public tracks play anonymously. Age-restricted, region-locked
  or sign-in-only tracks may return `UNPLAYABLE` from that client even when the signed-in
  library lists them.
- **AAC only.** Formats are filtered to `audio/mp4`; an Opus-only response fails with
  "No playable audio from InnerTube" rather than falling back.
- **No offline mode.** Tracks are not downloaded; the app does not bypass DRM.
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
