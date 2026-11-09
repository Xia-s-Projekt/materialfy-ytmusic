//! Social/Jam: not available in YTMusic (Spotify-only features stubbed).

use std::sync::Arc;

use serde::Serialize;

use tokio::runtime::Handle;
use tokio::sync::watch;
use yt_player::PlayerHandle;

use crate::auth::AuthService;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FriendActivity {
    pub user: String,
    pub user_uri: String,
    pub name: String,
    pub avatar: String,
    pub track: String,
    pub track_uri: String,
    pub uri: String,
    pub cover: String,
    pub artist: String,
    pub artist_uri: String,
    pub album: String,
    pub album_uri: String,
    pub context: String,
    pub context_uri: String,
    pub timestamp: i64,
    pub is_episode: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JamMember {
    pub id: String,
    pub username: String,
    pub name: String,
    pub avatar: String,
    pub is_host: bool,
    pub is_you: bool,
    pub listening: bool,
    pub controlling: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Jam {
    pub id: String,
    pub session_id: String,
    pub join_token: String,
    pub share_url: String,
    pub is_host: bool,
    pub host_name: String,
    pub members: Vec<JamMember>,
    pub host: String,
    pub host_device_id: String,
    pub max_members: u32,
    pub queue_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SocialSnapshot {
    pub friends: Arc<Vec<FriendActivity>>,
    pub friends_loaded: bool,
    pub friends_loading: bool,
    pub friends_error: String,
    pub jam: Option<Jam>,
    pub jam_checked: bool,
    pub jam_busy: bool,
    pub jam_error: String,
}

pub struct SocialService {
    _rt: Handle,
    _auth: Arc<AuthService>,
    _player: PlayerHandle,
    snapshot: watch::Sender<SocialSnapshot>,
}

impl SocialService {
    pub fn new(rt: Handle, auth: Arc<AuthService>, player: PlayerHandle) -> Arc<Self> {
        let (snapshot, _) = watch::channel(SocialSnapshot { jam_checked: true, ..Default::default() });
        Arc::new(Self { _rt: rt, _auth: auth, _player: player, snapshot })
    }
    pub fn refresh_friends(&self, _force: bool) {}
    pub fn refresh_jam(&self) {}
    pub fn start_jam(&self) {}
    pub fn join_jam(&self, _link: &str) {}
    pub fn leave_jam(&self) {}
    pub fn snapshot(&self) -> SocialSnapshot { self.snapshot.borrow().clone() }
    pub fn watch(&self) -> watch::Receiver<SocialSnapshot> { self.snapshot.subscribe() }
}
