//! Devices: local-only (no Spotify Connect in YTMusic fork).

use std::sync::Arc;

use md3_model::DeviceItem;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};

use mf_platform::SettingsStore;
use yt_player::PlayerHandle;

use crate::auth::AuthService;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DevicesSnapshot {
    pub devices: Arc<Vec<DeviceItem>>,
    pub active: Option<DeviceItem>,
    pub loading: bool,
    pub transferring_to: String,
    pub error_text: String,
    pub this_device_id: String,
    pub this_device_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DevicesEvent {
    Notice(String),
}

pub struct DevicesService {
    _rt: Handle,
    _auth: Arc<AuthService>,
    player: PlayerHandle,
    _settings: Arc<SettingsStore>,
    snapshot: watch::Sender<DevicesSnapshot>,
    events: broadcast::Sender<DevicesEvent>,
}

impl DevicesService {
    pub fn new(rt: Handle, auth: Arc<AuthService>, player: PlayerHandle, settings: Arc<SettingsStore>) -> Arc<Self> {
        let this = DeviceItem {
            id: "this-computer".into(),
            name: "This computer".into(),
            kind: "computer".into(),
            is_active: true,
            is_this_computer: true,
            is_restricted: false,
            supports_volume: true,
            volume: 80,
            source: "local".into(),
        };
        let snap = DevicesSnapshot {
            devices: Arc::new(vec![this.clone()]),
            active: Some(this.clone()),
            this_device_id: this.id.clone(),
            this_device_name: this.name.clone(),
            ..Default::default()
        };
        let (snapshot, _) = watch::channel(snap);
        let (events, _) = broadcast::channel(64);
        Arc::new(Self { _rt: rt, _auth: auth, player, _settings: settings, snapshot, events })
    }

    pub fn refresh(&self) {}
    pub fn transfer(&self, _device_id: String, _play: bool) {}
    pub fn transfer_here(&self) { self.player.transfer_here(true); }
    pub fn set_volume(&self, _device_id: String, percent: u8) {
        self.player.set_volume(f64::from(percent) / 100.0);
    }
    pub fn activate_receiver(&self, _name: String) {
        let _ = self.events.send(DevicesEvent::Notice(
            "Spotify Connect is not available in the YouTube Music build".into(),
        ));
    }
    pub fn snapshot(&self) -> DevicesSnapshot { self.snapshot.borrow().clone() }
    pub fn watch(&self) -> watch::Receiver<DevicesSnapshot> { self.snapshot.subscribe() }
    pub fn subscribe(&self) -> broadcast::Receiver<DevicesEvent> { self.events.subscribe() }
}
