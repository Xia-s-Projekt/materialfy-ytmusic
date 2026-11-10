//! MPRIS2 (org.mpris.MediaPlayer2.materialfy, /org/mpris/MediaPlayer2).
//! Linux only (zbus, the session bus); on other OSes `MprisServer::start` returns Err(Unsupported).
//!
//! The server knows nothing about the player: it reads [`MprisState`] from a watch channel and sends [`MprisCommand`] to an mpsc.
//! sf-services wires it to the player (`glue::start_media_controls`).
//! Properties/methods/signals — as in mpris.py from yamusic-md3: Identity "Materialfy", DesktopEntry "materialfy",
//! CanQuit/CanRaise true, HasTrackList false; Player: PlaybackStatus, LoopStatus (None/Playlist/Track),
//! Shuffle, Volume, Rate=1 (writes are ignored), Metadata (mpris:trackid /org/materialfy/track/<id with _ in place of non-alphanumerics>,
//! mpris:length, xesam:title, xesam:artist [split ","], xesam:album, xesam:url https://open.spotify.com/track/<id> (an episode is /episode/),
//! mpris:artUrl), Position, Min/MaxRate 1, CanGoNext/CanGoPrevious/CanPlay/CanPause/CanSeek = has_track, CanControl true;
//! PropertiesChanged — only for changed fields (the _sync rules in mpris.py); Seeked — on a position jump > 2 s.
//!
//! Writing LoopStatus/Shuffle/Volume from the bus is reflected immediately in the reported state (PropertiesChanged goes out
//! with the new value), and the command goes to the player; the confirmation from watch does not produce a second signal.
use tokio::sync::{mpsc, watch};

use crate::PlatformError;

pub const BUS_NAME: &str = "org.mpris.MediaPlayer2.materialfy";
pub const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
pub const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MprisState {
    pub has_track: bool,
    pub playing: bool,
    /// the track's base62 id or the URI `spotify:track|episode:<id>`
    pub track_id: String,
    pub title: String,
    /// "A, B" — split on commas into xesam:artist
    pub artist: String,
    pub album: String,
    /// file://<coverFile> or an https cover; empty — no artUrl
    pub art_url: String,
    pub duration_us: i64,
    pub position_us: i64,
    pub shuffle: bool,
    /// "off" | "all" | "one"
    pub repeat: String,
    pub volume: f64,
}

impl MprisState {
    /// The current track's object path (NO_TRACK if the id is empty).
    pub fn track_path(&self) -> String {
        let id: String = self
            .track_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        if id.is_empty() { NO_TRACK.to_owned() } else { format!("/org/materialfy/track/{id}") }
    }

    /// xesam:url — the open.spotify.com link; empty if there is no track.
    pub fn url(&self) -> String {
        let (kind, id) = match self.track_id.strip_prefix("spotify:").and_then(|rest| rest.split_once(':')) {
            Some((kind, id)) => (kind, id),
            None => ("track", self.track_id.as_str()),
        };
        if id.is_empty() { String::new() } else { format!("https://open.spotify.com/{kind}/{id}") }
    }

    pub fn playback_status(&self) -> &'static str {
        if self.playing {
            "Playing"
        } else if self.has_track {
            "Paused"
        } else {
            "Stopped"
        }
    }

    pub fn loop_status(&self) -> &'static str {
        match self.repeat.as_str() {
            "all" => "Playlist",
            "one" => "Track",
            _ => "None",
        }
    }

    pub fn artists(&self) -> Vec<String> {
        self.artist
            .split(',')
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// The Player property names to report as changed on the transition old → self (like _sync).
    pub fn changed_properties(&self, old: &MprisState) -> Vec<&'static str> {
        let mut changed = Vec::new();
        if (old.playing, old.has_track) != (self.playing, self.has_track) {
            changed.push("PlaybackStatus");
        }
        if old.track_id != self.track_id
            || old.title != self.title
            || old.artist != self.artist
            || old.album != self.album
            || old.art_url != self.art_url
            || old.duration_us != self.duration_us
        {
            changed.extend(["Metadata", "CanGoNext", "CanGoPrevious", "CanPlay", "CanPause", "CanSeek"]);
        }
        if old.shuffle != self.shuffle {
            changed.push("Shuffle");
        }
        if old.repeat != self.repeat {
            changed.push("LoopStatus");
        }
        if (old.volume - self.volume).abs() > 1e-3 {
            changed.push("Volume");
        }
        changed
    }

    /// A position jump greater than 2 s — the Seeked signal.
    pub fn seeked_from(&self, old: &MprisState) -> bool {
        (self.position_us - old.position_us).abs() > 2_000_000
    }
}

/// MPRIS LoopStatus → the player's repeat mode.
pub fn repeat_from_loop_status(value: &str) -> &'static str {
    match value {
        "Playlist" => "all",
        "Track" => "one",
        _ => "off",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MprisCommand {
    Raise,
    Quit,
    Next,
    Previous,
    Pause,
    PlayPause,
    /// Stop — like Pause
    Stop,
    Play,
    /// absolute position, µs (Seek(offset) is recomputed in the server from position_us)
    SeekTo(i64),
    /// "off" | "all" | "one"
    SetRepeat(String),
    SetShuffle(bool),
    SetVolume(f64),
}

pub struct MprisServer {
    #[cfg(target_os = "linux")]
    inner: imp::Server,
}

impl MprisServer {
    /// Connect to the session bus, export the objects, take BUS_NAME and watch `state`.
    pub async fn start(
        state: watch::Receiver<MprisState>,
        commands: mpsc::UnboundedSender<MprisCommand>,
    ) -> Result<MprisServer, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            Ok(MprisServer { inner: imp::Server::start(state, commands).await? })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (state, commands);
            Err(PlatformError::Unsupported)
        }
    }

    /// Disconnect from the bus.
    pub async fn close(self) {
        #[cfg(target_os = "linux")]
        self.inner.close().await;
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::borrow::Cow;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, MutexGuard};

    use tokio::sync::{mpsc, watch};
    use tokio::task::JoinHandle;
    use zbus::names::InterfaceName;
    use zbus::object_server::SignalEmitter;
    use zbus::zvariant::{ObjectPath, OwnedValue, Value};
    use zbus::{Connection, interface};

    use super::{BUS_NAME, MprisCommand, MprisState, NO_TRACK, OBJECT_PATH, repeat_from_loop_status};
    use crate::PlatformError;

    const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

    type Shared = Arc<Mutex<MprisState>>;
    type Commands = mpsc::UnboundedSender<MprisCommand>;

    fn lock(shared: &Shared) -> MutexGuard<'_, MprisState> {
        shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn dbus_err(err: zbus::Error) -> PlatformError {
        PlatformError::DBus(err.to_string())
    }

    fn owned(value: Value<'_>) -> OwnedValue {
        // our values contain no file descriptors — an error is impossible
        value.try_into_owned().unwrap_or_else(|_| OwnedValue::from(0u8))
    }

    fn object_path(path: String) -> Value<'static> {
        match ObjectPath::try_from(path) {
            Ok(p) => Value::from(p),
            Err(_) => Value::from(ObjectPath::from_static_str_unchecked(NO_TRACK)),
        }
    }

    fn metadata(s: &MprisState) -> HashMap<String, OwnedValue> {
        let mut meta = HashMap::new();
        if !s.has_track {
            meta.insert("mpris:trackid".into(), owned(object_path(NO_TRACK.into())));
            return meta;
        }
        meta.insert("mpris:trackid".into(), owned(object_path(s.track_path())));
        meta.insert("mpris:length".into(), owned(Value::from(s.duration_us)));
        meta.insert("xesam:title".into(), owned(Value::from(s.title.as_str())));
        meta.insert("xesam:artist".into(), owned(Value::from(s.artists())));
        meta.insert("xesam:album".into(), owned(Value::from(s.album.as_str())));
        let url = s.url();
        if !url.is_empty() {
            meta.insert("xesam:url".into(), owned(Value::from(url)));
        }
        if !s.art_url.is_empty() {
            meta.insert("mpris:artUrl".into(), owned(Value::from(s.art_url.as_str())));
        }
        meta
    }

    /// A Player property's value by name (for PropertiesChanged).
    fn player_property(s: &MprisState, name: &str) -> Option<Value<'static>> {
        Some(match name {
            "PlaybackStatus" => Value::from(s.playback_status()),
            "LoopStatus" => Value::from(s.loop_status()),
            "Shuffle" => Value::from(s.shuffle),
            "Volume" => Value::from(s.volume),
            "Metadata" => Value::from(metadata(s)),
            "CanGoNext" | "CanGoPrevious" | "CanPlay" | "CanPause" | "CanSeek" => Value::from(s.has_track),
            _ => return None,
        })
    }

    struct Root {
        commands: Commands,
    }

    #[interface(name = "org.mpris.MediaPlayer2")]
    impl Root {
        #[zbus(name = "Raise")]
        fn raise_window(&self) {
            let _ = self.commands.send(MprisCommand::Raise);
        }

        fn quit(&self) {
            let _ = self.commands.send(MprisCommand::Quit);
        }

        #[zbus(property)]
        fn can_quit(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn can_raise(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn has_track_list(&self) -> bool {
            false
        }

        #[zbus(property)]
        fn identity(&self) -> String {
            "Materialfy".into()
        }

        #[zbus(property)]
        fn desktop_entry(&self) -> String {
            "materialfy".into()
        }

        #[zbus(property)]
        fn supported_uri_schemes(&self) -> Vec<String> {
            Vec::new()
        }

        #[zbus(property)]
        fn supported_mime_types(&self) -> Vec<String> {
            Vec::new()
        }
    }

    struct Player {
        state: Shared,
        commands: Commands,
    }

    impl Player {
        fn send(&self, command: MprisCommand) {
            let _ = self.commands.send(command);
        }
    }

    #[interface(name = "org.mpris.MediaPlayer2.Player")]
    impl Player {
        fn next(&self) {
            self.send(MprisCommand::Next);
        }

        fn previous(&self) {
            self.send(MprisCommand::Previous);
        }

        fn pause(&self) {
            self.send(MprisCommand::Pause);
        }

        fn play_pause(&self) {
            self.send(MprisCommand::PlayPause);
        }

        fn stop(&self) {
            self.send(MprisCommand::Stop);
        }

        fn play(&self) {
            self.send(MprisCommand::Play);
        }

        fn seek(&self, offset: i64) {
            let target = lock(&self.state).position_us.saturating_add(offset).max(0);
            self.send(MprisCommand::SeekTo(target));
        }

        fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
            if track_id.as_str() == lock(&self.state).track_path() {
                self.send(MprisCommand::SeekTo(position.max(0)));
            }
        }

        fn open_uri(&self, _uri: &str) {}

        #[zbus(signal)]
        async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

        #[zbus(property)]
        fn playback_status(&self) -> String {
            lock(&self.state).playback_status().into()
        }

        #[zbus(property)]
        fn loop_status(&self) -> String {
            lock(&self.state).loop_status().into()
        }

        #[zbus(property)]
        fn set_loop_status(&self, value: String) {
            let mode = repeat_from_loop_status(&value);
            lock(&self.state).repeat = mode.into();
            self.send(MprisCommand::SetRepeat(mode.into()));
        }

        #[zbus(property)]
        fn shuffle(&self) -> bool {
            lock(&self.state).shuffle
        }

        #[zbus(property)]
        fn set_shuffle(&self, value: bool) {
            lock(&self.state).shuffle = value;
            self.send(MprisCommand::SetShuffle(value));
        }

        #[zbus(property)]
        fn volume(&self) -> f64 {
            lock(&self.state).volume
        }

        #[zbus(property)]
        fn set_volume(&self, value: f64) {
            let value = if value.is_nan() { 0.0 } else { value.clamp(0.0, 1.0) };
            lock(&self.state).volume = value;
            self.send(MprisCommand::SetVolume(value));
        }

        #[zbus(property)]
        fn rate(&self) -> f64 {
            1.0
        }

        #[zbus(property)]
        fn set_rate(&self, _value: f64) {}

        #[zbus(property)]
        fn metadata(&self) -> HashMap<String, OwnedValue> {
            metadata(&lock(&self.state))
        }

        #[zbus(property(emits_changed_signal = "false"))]
        fn position(&self) -> i64 {
            lock(&self.state).position_us
        }

        #[zbus(property)]
        fn minimum_rate(&self) -> f64 {
            1.0
        }

        #[zbus(property)]
        fn maximum_rate(&self) -> f64 {
            1.0
        }

        #[zbus(property)]
        fn can_go_next(&self) -> bool {
            lock(&self.state).has_track
        }

        #[zbus(property)]
        fn can_go_previous(&self) -> bool {
            lock(&self.state).has_track
        }

        #[zbus(property)]
        fn can_play(&self) -> bool {
            lock(&self.state).has_track
        }

        #[zbus(property)]
        fn can_pause(&self) -> bool {
            lock(&self.state).has_track
        }

        #[zbus(property)]
        fn can_seek(&self) -> bool {
            lock(&self.state).has_track
        }

        #[zbus(property)]
        fn can_control(&self) -> bool {
            true
        }
    }

    pub struct Server {
        conn: Connection,
        task: JoinHandle<()>,
    }

    impl Server {
        pub async fn start(
            mut state: watch::Receiver<MprisState>,
            commands: Commands,
        ) -> Result<Server, PlatformError> {
            let shared: Shared = Arc::new(Mutex::new(state.borrow_and_update().clone()));
            let conn = zbus::connection::Builder::session()
                .map_err(dbus_err)?
                .serve_at(OBJECT_PATH, Root { commands: commands.clone() })
                .map_err(dbus_err)?
                .serve_at(OBJECT_PATH, Player { state: shared.clone(), commands })
                .map_err(dbus_err)?
                .build()
                .await
                .map_err(dbus_err)?;
            // like dbus-fast: a taken name (a second instance with --new-instance) is not an error
            if let Err(err) = conn.request_name(BUS_NAME).await {
                tracing::warn!("MPRIS: could not acquire name {BUS_NAME}: {err}");
            }
            let emitter = SignalEmitter::new(&conn, OBJECT_PATH).map_err(dbus_err)?.into_owned();
            let task = tokio::spawn(async move {
                while state.changed().await.is_ok() {
                    let new = state.borrow_and_update().clone();
                    let old = std::mem::replace(&mut *lock(&shared), new.clone());
                    sync(&emitter, &old, &new).await;
                }
            });
            Ok(Server { conn, task })
        }

        pub async fn close(self) {
            self.task.abort();
            let _ = self.conn.release_name(BUS_NAME).await;
            let _ = self.conn.close().await;
        }
    }

    async fn sync(emitter: &SignalEmitter<'_>, old: &MprisState, new: &MprisState) {
        let changed: HashMap<&str, Value<'_>> = new
            .changed_properties(old)
            .into_iter()
            .filter_map(|name| player_property(new, name).map(|v| (name, v)))
            .collect();
        if !changed.is_empty() {
            let iface = InterfaceName::from_static_str_unchecked(PLAYER_IFACE);
            if let Err(err) =
                zbus::fdo::Properties::properties_changed(emitter, iface, changed, Cow::Borrowed(&[])).await
            {
                tracing::debug!("MPRIS PropertiesChanged: {err}");
            }
        }
        if new.seeked_from(old)
            && let Err(err) = Player::seeked(emitter, new.position_us).await
        {
            tracing::debug!("MPRIS Seeked: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> MprisState {
        MprisState {
            has_track: true,
            playing: true,
            track_id: "123:45".into(),
            title: "Song".into(),
            artist: "A, B ,".into(),
            album: "Album".into(),
            duration_us: 180_000_000,
            repeat: "off".into(),
            volume: 0.7,
            ..Default::default()
        }
    }

    #[test]
    fn derived_values() {
        let s = track();
        assert_eq!(s.track_path(), "/org/materialfy/track/123_45");
        assert_eq!(MprisState::default().track_path(), NO_TRACK);
        assert_eq!(s.artists(), ["A", "B"]);
        assert_eq!(s.playback_status(), "Playing");
        assert_eq!(MprisState { playing: false, ..track() }.playback_status(), "Paused");
        assert_eq!(MprisState::default().playback_status(), "Stopped");
        assert_eq!(MprisState { repeat: "all".into(), ..track() }.loop_status(), "Playlist");
        assert_eq!(MprisState { repeat: "one".into(), ..track() }.loop_status(), "Track");
        assert_eq!(s.loop_status(), "None");
        assert_eq!(repeat_from_loop_status("Track"), "one");
        assert_eq!(repeat_from_loop_status("bogus"), "off");
        assert_eq!(
            MprisState { track_id: "4uLU6hMCjMI75M1A2tKUQC".into(), ..track() }.url(),
            "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"
        );
        assert_eq!(
            MprisState { track_id: "spotify:episode:ep1".into(), ..track() }.url(),
            "https://open.spotify.com/episode/ep1"
        );
        assert_eq!(MprisState::default().url(), "");
    }

    #[test]
    fn diff_rules() {
        let old = track();
        assert!(old.changed_properties(&old).is_empty());
        let paused = MprisState { playing: false, ..track() };
        assert_eq!(paused.changed_properties(&old), ["PlaybackStatus"]);
        let next = MprisState { track_id: "9".into(), ..track() };
        assert_eq!(
            next.changed_properties(&old),
            ["Metadata", "CanGoNext", "CanGoPrevious", "CanPlay", "CanPause", "CanSeek"]
        );
        let quiet = MprisState { volume: 0.7005, position_us: 1_000_000, ..track() };
        assert!(quiet.changed_properties(&old).is_empty());
        assert!(!quiet.seeked_from(&old));
        let jumped = MprisState { position_us: 60_000_000, ..track() };
        assert!(jumped.seeked_from(&old));
        let modes = MprisState { shuffle: true, repeat: "all".into(), volume: 0.2, ..track() };
        assert_eq!(modes.changed_properties(&old), ["Shuffle", "LoopStatus", "Volume"]);
    }

    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn unsupported_elsewhere() {
        let (_tx, rx) = watch::channel(MprisState::default());
        let (cmd, _crx) = mpsc::unbounded_channel();
        assert!(matches!(MprisServer::start(rx, cmd).await, Err(PlatformError::Unsupported)));
    }
}
