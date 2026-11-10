//! Spotify Connect (provider "services-extra"): device list (/me/player/devices, freshness 12 s) +
//! receivers from the local network (zeroconf, not yet in the account), transfer (PlayerHandle::transfer_*),
//! device volume, receiver activation (addUser → wait for it to appear ≤ 20 s → transfer), this device's name.
//! The active device and "a transfer is in progress" — from the player snapshot (it keeps the optimistic state and reports itself
//! "{name} did not answer"). The logic is ported from spotifast (src/ui/devices.rs).
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use md3_model::DeviceItem;
use sf_api::Operation;
use sf_player::engine::zeroconf::{self, Receiver};
use sf_player::{PlayerEvent, PlayerHandle};
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use mf_platform::{SettingKey, SettingsStore};

use crate::ServiceError;
use crate::auth::{AuthEvent, AuthService};
use crate::extra::connect::{self, SOURCE_ZEROCONF, ThisDevice};
use crate::lock;

/// The device list is considered fresh (reference app.rs DEVICES_FRESH).
pub const DEVICES_FRESH: Duration = Duration::from_secs(12);
/// mDNS receiver search.
pub const DISCOVER_TIMEOUT: Duration = Duration::from_secs(3);
/// Wait until the activated receiver appears in Spotify Connect.
pub const RECEIVER_WAIT: Duration = Duration::from_secs(20);
const RECEIVER_POLL: Duration = Duration::from_secs(2);
/// The volume goes out no more often when dragging.
const VOLUME_DEBOUNCE: Duration = Duration::from_millis(150);
/// The shown volume is held until the list answers.
const VOLUME_HOLD: Duration = Duration::from_millis(2500);
/// The name we introduce ourselves to the receiver with at addUser.
const CLIENT_NAME: &str = "Materialfy";
const NEED_PLAYBACK: &str = "Turn on playback on this computer first — the receiver needs something to connect to";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DevicesSnapshot {
    /// active first, then this PC, then the rest; zeroconf receivers — source = "zeroconf"
    pub devices: Arc<Vec<DeviceItem>>,
    pub active: Option<DeviceItem>,
    pub loading: bool,
    /// id of the device the transfer is going to ("" — none)
    pub transferring_to: String,
    pub error_text: String,
    /// id and name of this PC's Connect device (empty until the engine is ready)
    pub this_device_id: String,
    pub this_device_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DevicesEvent {
    /// receiver activation errors etc.; "{name} did not answer" is sent only by the player (PlayerEvent::Notice)
    Notice(String),
}

#[derive(Default)]
struct State {
    api: Vec<DeviceItem>,
    receivers: Vec<Receiver>,
    fetched_at: Option<Instant>,
    api_in_flight: bool,
    discover_in_flight: bool,
    error_text: String,
    /// the target of the last transfer
    transfer_target: String,
    /// the receiver row that is connecting
    activating: Option<String>,
    /// optimistic volume: id → (percent, when)
    volumes: HashMap<String, (i32, Instant)>,
    volume_seq: HashMap<String, u64>,
    /// login generation: answers before logout are not accepted
    account_generation: u64,
}

pub struct DevicesService {
    rt: Handle,
    auth: Arc<AuthService>,
    player: PlayerHandle,
    settings: Arc<SettingsStore>,
    snapshot: watch::Sender<DevicesSnapshot>,
    events: broadcast::Sender<DevicesEvent>,
    state: Mutex<State>,
    me: Weak<DevicesService>,
}

impl DevicesService {
    pub fn new(rt: Handle, auth: Arc<AuthService>, player: PlayerHandle, settings: Arc<SettingsStore>) -> Arc<Self> {
        let service = Arc::new_cyclic(|me| Self {
            rt: rt.clone(),
            auth,
            player,
            settings,
            snapshot: watch::channel(DevicesSnapshot::default()).0,
            events: broadcast::channel(64).0,
            state: Mutex::new(State::default()),
            me: me.clone(),
        });
        rt.spawn(listen(Arc::downgrade(&service)));
        service
    }

    /// Request the list (Web API + mDNS); called when the menu opens and on refresh.
    pub fn refresh(&self) {
        if !self.auth.is_signed_in() {
            lock(&self.state).error_text = ServiceError::NotSignedIn.to_string();
            self.recompute();
            return;
        }
        let (fetch_api, discover, generation) = {
            let mut st = lock(&self.state);
            let fresh = st.fetched_at.is_some_and(|at| at.elapsed() < DEVICES_FRESH);
            let fetch_api = !fresh && !st.api_in_flight;
            let discover = !fresh && !st.discover_in_flight;
            st.api_in_flight |= fetch_api;
            st.discover_in_flight |= discover;
            (fetch_api, discover, st.account_generation)
        };
        self.recompute();
        let Some(me) = self.me.upgrade() else { return };
        if fetch_api {
            let me = me.clone();
            self.rt.spawn(async move {
                let result = me.fetch_devices().await;
                {
                    let mut st = lock(&me.state);
                    if st.account_generation != generation {
                        return;
                    }
                    st.api_in_flight = false;
                    match result {
                        Ok(devices) => {
                            st.api = devices;
                            st.error_text.clear();
                            st.fetched_at = Some(Instant::now());
                        }
                        // the previous list stays, the error is visible in the menu
                        Err(e) => st.error_text = mf_i18n::tf("Couldn’t get the device list: {0}", &[&e]),
                    }
                }
                me.recompute();
            });
        }
        if discover {
            self.rt.spawn(async move {
                let found = tokio::task::spawn_blocking(|| {
                    zeroconf::discover(DISCOVER_TIMEOUT).and_then(zeroconf::resolve_receivers)
                })
                .await;
                let receivers = match found {
                    Ok(Ok(receivers)) => Some(receivers),
                    Ok(Err(e)) => {
                        tracing::debug!("no receivers found on the network: {e:#}");
                        None
                    }
                    Err(e) => {
                        tracing::warn!("receiver discovery failed: {e}");
                        None
                    }
                };
                {
                    let mut st = lock(&me.state);
                    if st.account_generation != generation {
                        return;
                    }
                    st.discover_in_flight = false;
                    if let Some(receivers) = receivers {
                        st.receivers = receivers;
                    }
                }
                me.recompute();
            });
        }
    }

    /// Transfer playback to a device (this PC's id → transfer_here).
    pub fn transfer(&self, device_id: String, play: bool) {
        let row = self.snapshot().devices.iter().find(|d| d.id == device_id).cloned();
        if let Some(row) = row.filter(|d| d.source == SOURCE_ZEROCONF) {
            self.activate_receiver(row.name);
            return;
        }
        {
            let mut st = lock(&self.state);
            st.transfer_target = device_id.clone();
            // after the transfer the is_active flags in the Web API cache are stale
            st.fetched_at = None;
        }
        if self.player.this_device_id().as_deref() == Some(device_id.as_str()) {
            self.player.transfer_here(play);
        } else {
            self.player.transfer_to(device_id, play);
        }
        self.recompute();
    }

    pub fn transfer_here(&self) {
        let Some(id) = self.player.this_device_id() else {
            self.player.transfer_here(true);
            return;
        };
        self.transfer(id, true);
    }

    /// Device volume 0..100 (with a 150 ms delay when dragging).
    pub fn set_volume(&self, device_id: String, percent: u8) {
        let percent = percent.min(100);
        let active = self.player.snapshot().device.map(|d| d.id).unwrap_or_default();
        let this = self.player.this_device_id().unwrap_or_default();
        let seq = {
            let mut st = lock(&self.state);
            st.volumes.insert(device_id.clone(), (i32::from(percent), Instant::now()));
            let seq = st.volume_seq.entry(device_id.clone()).or_default();
            *seq += 1;
            *seq
        };
        self.recompute();
        // the playing device and this PC — through the player: it keeps the shown volume itself
        if device_id == active || device_id == this {
            self.player.set_volume(f64::from(percent) / 100.0);
            return;
        }
        let Some(me) = self.me.upgrade() else { return };
        self.rt.spawn(async move {
            tokio::time::sleep(VOLUME_DEBOUNCE).await;
            if lock(&me.state).volume_seq.get(&device_id) != Some(&seq) {
                return;
            }
            // first through the session (connect-state), Web API — only if there is none or it refused
            let connect = me.player.connect_volume(&device_id, percent).await;
            if let Some(Err(e)) = &connect {
                tracing::debug!("volume via session failed ({e}); trying Web API");
            }
            let result = match connect {
                Some(Ok(())) => Ok(()),
                _ => async {
                    let client = me.auth.api().client_for(Operation::Playback).await?;
                    client.set_volume(percent, Some(device_id.as_str())).await
                }
                .await
                .map_err(ServiceError::from),
            };
            if let Err(e) = result {
                let name = me.snapshot().devices.iter().find(|d| d.id == device_id).map(|d| d.name.clone()).unwrap_or_default();
                {
                    let mut st = lock(&me.state);
                    if st.volume_seq.get(&device_id) == Some(&seq) {
                        st.volumes.remove(&device_id);
                    }
                }
                me.recompute();
                me.notice(mf_i18n::tf("Couldn’t change the volume of “{0}”: {1}", &[&name, &e]));
            }
        });
    }

    /// Connect a receiver from the local network by name (zeroconf addUser), then transfer.
    pub fn activate_receiver(&self, name: String) {
        let receiver = lock(&self.state).receivers.iter().find(|r| r.name == name).cloned();
        let Some(receiver) = receiver else {
            self.notice(mf_i18n::tf("“{0}” is no longer visible on the network", &[&name]));
            return;
        };
        let Some(session) = self.player.session() else {
            self.notice(mf_i18n::t(NEED_PLAYBACK));
            return;
        };
        let credentials = zeroconf::Credentials {
            username: session.username(),
            // AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS: reusable session credentials
            auth_type: 1,
            auth_data: session.auth_data(),
        };
        if credentials.username.is_empty() || credentials.auth_data.is_empty() {
            self.notice(mf_i18n::t(NEED_PLAYBACK));
            return;
        }
        let row = connect::receiver_row_id(&receiver);
        let generation = {
            let mut st = lock(&self.state);
            if st.activating.is_some() {
                return;
            }
            st.activating = Some(row);
            st.account_generation
        };
        self.recompute();
        let Some(me) = self.me.upgrade() else { return };
        self.rt.spawn(async move {
            let result = me.activate(receiver.clone(), credentials, generation).await;
            {
                let mut st = lock(&me.state);
                if st.account_generation != generation {
                    return;
                }
                st.activating = None;
            }
            match result {
                Ok(device_id) => {
                    lock(&me.state).transfer_target = device_id.clone();
                    me.player.transfer_to(device_id, true);
                }
                Err(text) => me.notice(text),
            }
            me.recompute();
        });
    }

    /// This device's default name: "Materialfy · <PC name>".
    pub fn default_device_name() -> String {
        let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default();
        if host.is_empty() { "Materialfy".to_owned() } else { format!("Materialfy · {host}") }
    }

    pub fn snapshot(&self) -> DevicesSnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn watch(&self) -> watch::Receiver<DevicesSnapshot> {
        self.snapshot.subscribe()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DevicesEvent> {
        self.events.subscribe()
    }

    // --- internal ----------------------------------------------------------------------------

    fn notice(&self, text: String) {
        let _ = self.events.send(DevicesEvent::Notice(text));
    }

    fn device_name(&self) -> String {
        let name = self.settings.get_str(SettingKey::DeviceName);
        if name.trim().is_empty() { Self::default_device_name() } else { name }
    }

    /// The list from the session's connect-state cluster; Web API — only without the engine or when the session refuses.
    async fn fetch_devices(&self) -> Result<Vec<DeviceItem>, ServiceError> {
        let this = self.player.this_device_id();
        let devices = match self.player.connect_devices().await {
            Some(devices) => devices,
            None => self.auth.api().client_for(Operation::Playback).await?.devices().await?,
        };
        Ok(devices.iter().map(|d| sf_api::map::device_item(d, this.as_deref())).collect())
    }

    /// The cluster changed (a session push): re-read the list from it — without the network, while the cluster is fresh.
    fn refresh_connect(&self) {
        let generation = lock(&self.state).account_generation;
        let Some(me) = self.me.upgrade() else { return };
        self.rt.spawn(async move {
            let Some(devices) = me.player.connect_devices().await else { return };
            let this = me.player.this_device_id();
            {
                let mut st = lock(&me.state);
                if st.account_generation != generation {
                    return;
                }
                st.api = devices.iter().map(|d| sf_api::map::device_item(d, this.as_deref())).collect();
                st.error_text.clear();
                st.fetched_at = Some(Instant::now());
            }
            me.recompute();
        });
    }

    /// addUser on the receiver, then wait until Spotify shows it in the device list; Ok — the device id.
    async fn activate(&self, receiver: Receiver, credentials: zeroconf::Credentials, generation: u64) -> Result<String, String> {
        let name = receiver.name.clone();
        let our_name = CLIENT_NAME.to_owned();
        let handed = tokio::task::spawn_blocking({
            let receiver = receiver.clone();
            move || -> Result<zeroconf::Info, String> {
                // receiver addresses — the local network: past the proxy
                let http = reqwest::blocking::Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(8))
                    .build()
                    .map_err(|e| e.to_string())?;
                let info = zeroconf::get_info(&http, &receiver).map_err(|e| format!("{e:#}"))?;
                zeroconf::add_user(&http, &receiver, &info, &credentials, &our_name).map_err(|e| format!("{e:#}"))?;
                Ok(info)
            }
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r)
        .map_err(|e| mf_i18n::tf("Couldn’t connect “{0}”: {1}", &[&name, &e]))?;
        let wanted_id = receiver.device_id.clone().filter(|id| !id.is_empty()).or_else(|| {
            let id = handed.device_id.clone();
            (!id.is_empty()).then_some(id)
        });
        let deadline = Instant::now() + RECEIVER_WAIT;
        loop {
            if lock(&self.state).account_generation != generation {
                return Err(String::new());
            }
            if let Ok(devices) = self.fetch_devices().await {
                let found = devices
                    .iter()
                    .find(|d| match wanted_id.as_deref() {
                        Some(id) => d.id == id,
                        None => d.name == name,
                    })
                    .map(|d| d.id.clone());
                {
                    let mut st = lock(&self.state);
                    st.api = devices;
                    st.fetched_at = Some(Instant::now());
                }
                self.recompute();
                if let Some(id) = found.filter(|id| !id.is_empty()) {
                    return Ok(id);
                }
            }
            if Instant::now() >= deadline {
                return Err(mf_i18n::tf("“{0}” didn’t show up in Spotify Connect — try again", &[&name]));
            }
            tokio::time::sleep(RECEIVER_POLL).await;
        }
    }

    /// A snapshot from the list cache + the player snapshot (active device, transfer, this PC's volume).
    fn recompute(&self) {
        let p = self.player.snapshot();
        let this_id = self.player.this_device_id().unwrap_or_default();
        let this_name = if this_id.is_empty() { String::new() } else { self.device_name() };
        let mut st = lock(&self.state);
        st.volumes.retain(|_, (_, at)| at.elapsed() < VOLUME_HOLD);
        let active_id = match &p.device {
            Some(d) => d.id.clone(),
            None if p.is_local && p.has_track() => this_id.clone(),
            None => String::new(),
        };
        let this = (!this_id.is_empty()).then(|| ThisDevice {
            id: this_id.clone(),
            name: this_name.clone(),
            volume: if p.is_local { (p.volume * 100.0).round() as i32 } else { -1 },
        });
        let mut devices = connect::merge(&st.api, &st.receivers, this.as_ref(), &active_id);
        for d in &mut devices {
            if d.is_this_computer && p.is_local {
                d.volume = (p.volume * 100.0).round() as i32;
            }
            if let Some((volume, _)) = st.volumes.get(&d.id) {
                d.volume = *volume;
            }
        }
        let active = devices.iter().find(|d| d.is_active).cloned().or_else(|| p.device.clone());
        let transferring_to = match &st.activating {
            Some(row) => row.clone(),
            None if p.transferring => st.transfer_target.clone(),
            None => String::new(),
        };
        let next = DevicesSnapshot {
            devices: Arc::new(devices),
            active,
            loading: st.api_in_flight || st.discover_in_flight,
            transferring_to,
            error_text: st.error_text.clone(),
            this_device_id: this_id,
            this_device_name: this_name,
        };
        drop(st);
        self.snapshot.send_if_modified(|s| {
            if *s == next {
                return false;
            }
            *s = next;
            true
        });
    }

    fn reset(&self) {
        {
            let mut st = lock(&self.state);
            let generation = st.account_generation + 1;
            *st = State { account_generation: generation, ..State::default() };
        }
        self.recompute();
    }
}

/// Player (active device, transfer, volume), login and device name → snapshot.
async fn listen(weak: Weak<DevicesService>) {
    let Some(me) = weak.upgrade() else { return };
    let mut player = me.player.subscribe().events;
    let mut auth = me.auth.subscribe();
    let mut settings = me.settings.subscribe();
    let mut connect = me.player.connect_changes();
    me.recompute();
    drop(me);
    loop {
        tokio::select! {
            changed = connect.changed() => {
                if changed.is_err() {
                    break;
                }
                let Some(me) = weak.upgrade() else { break };
                me.refresh_connect();
            },
            event = player.recv() => match event {
                Ok(PlayerEvent::DeviceChanged | PlayerEvent::LocalChanged | PlayerEvent::VolumeChanged | PlayerEvent::TrackChanged)
                | Err(broadcast::error::RecvError::Lagged(_)) => {
                    let Some(me) = weak.upgrade() else { break };
                    me.recompute();
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            event = auth.recv() => {
                let Some(me) = weak.upgrade() else { break };
                match event {
                    Ok(AuthEvent::SignedOut) => me.reset(),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => me.recompute(),
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            },
            key = settings.recv() => match key {
                Ok(SettingKey::DeviceName) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    let Some(me) = weak.upgrade() else { break };
                    me.recompute();
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
}
