//! The host of the local engine: connecting, restarting while carrying over playback, recovery
//! after a session drop, and stopping. It reports the state to the actor via Msg::Local.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::runtime::Handle;
use tokio::sync::mpsc;

use super::actor::{LocalMsg, Msg};
use super::backend::{EngineLink, LocalEngine};
use crate::engine::{Engine, EngineConfig, EngineEvent, LoadSpec, Notify, PlaybackResume};
use crate::{Credentials, EngineStarted};

/// How many consecutive session drops we tolerate within ten minutes.
const RECONNECT_LIMIT: usize = 5;
const RECONNECT_WINDOW: Duration = Duration::from_secs(600);

/// An English translation key; outward it is `mf_i18n::t(PREMIUM_REQUIRED)` (both languages contain "Premium": by it
/// sf-services recognizes the refusal).
pub(crate) const PREMIUM_REQUIRED: &str = "Spotify Premium required";

pub(crate) struct Slot {
    pub engine: Arc<EngineLink>,
    pub config: EngineConfig,
    pub credentials: Credentials,
}

pub(crate) struct EngineHost {
    slot: Mutex<Option<Slot>>,
    tx: mpsc::UnboundedSender<Msg>,
    reconnects: Mutex<Vec<Instant>>,
    /// connection generation: a stale connect does not take the slot
    generation: Mutex<u64>,
}

impl EngineHost {
    pub fn new(tx: mpsc::UnboundedSender<Msg>) -> Self {
        Self { slot: Mutex::new(None), tx, reconnects: Mutex::new(Vec::new()), generation: Mutex::new(0) }
    }

    pub fn engine(&self) -> Option<Arc<EngineLink>> {
        self.slot.lock().ok()?.as_ref().map(|slot| slot.engine.clone())
    }

    fn take(&self) -> Option<Slot> {
        self.slot.lock().ok()?.take()
    }

    fn next_generation(&self) -> u64 {
        let mut generation = self.generation.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *generation += 1;
        *generation
    }

    fn current_generation(&self) -> u64 {
        *self.generation.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Connect the engine; the previous one is stopped.
    pub async fn connect(
        self: &Arc<Self>,
        credentials: Credentials,
        config: EngineConfig,
        resume: Option<PlaybackResume>,
    ) -> Result<EngineStarted, String> {
        let generation = self.next_generation();
        if let Some(old) = self.take() {
            old.engine.engine.shutdown();
        }
        let _ = self.tx.send(Msg::Local(LocalMsg::Connecting));
        let notify: Notify = {
            let tx = self.tx.clone();
            Arc::new(move |event: EngineEvent| {
                let _ = tx.send(Msg::Engine(Box::new(event)));
            })
        };
        let result = async {
            let cache = config.open_cache().map_err(|error| format!("{error:#}"))?;
            Engine::connect(&config, config.proxy.librespot_url(), credentials.clone(), cache, notify)
                .await
                .map_err(|error| format!("{error:#}"))
        }
        .await;
        let engine = match result {
            Ok(engine) => engine,
            Err(error) => {
                // the engine errors are already translated (friendly_connect_error), and "Premium" exists in both languages
                let text = if error.to_lowercase().contains("premium") { mf_i18n::t(PREMIUM_REQUIRED) } else { error };
                tracing::warn!("local playback didn't start: {text}");
                if generation == self.current_generation() {
                    let _ = self.tx.send(Msg::Local(LocalMsg::Failed(text.clone())));
                }
                return Err(text);
            }
        };
        if generation != self.current_generation() {
            engine.shutdown();
            return Err(mf_i18n::t("Playback setup cancelled"));
        }
        let device_id = engine.device_id().to_owned();
        let reusable = engine.credentials();
        let link = Arc::new(EngineLink { engine, tap: config.tap.clone() });
        if let Some(resume) = resume {
            resume_playback(link.clone(), resume);
        }
        let ready = LocalMsg::Ready {
            engine: link.clone() as Arc<dyn LocalEngine>,
            device_id: device_id.clone(),
            name: config.device_name.clone(),
            bitrate: config.bitrate_kbps,
            autoplay: config.autoplay,
        };
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(Slot {
                engine: link,
                config,
                credentials: reusable.clone().unwrap_or(credentials),
            });
        }
        let _ = self.tx.send(Msg::Local(ready));
        Ok(EngineStarted { device_id, reusable })
    }

    /// Restart with new settings (or the previous ones), carrying over the track and position.
    /// `load` — what to load into the new engine instead of a single track (context, list, modes — from the core);
    /// a dropped librespot session is still restored from its own snapshot.
    pub fn restart(self: &Arc<Self>, rt: &Handle, config: Option<EngineConfig>, volume: u16, load: Option<LoadSpec>) {
        let Some(old) = self.take() else {
            return;
        };
        let resume = match old.engine.engine.resume_point() {
            Some(PlaybackResume::Session(snapshot)) => Some(PlaybackResume::Session(snapshot)),
            other => load.map(PlaybackResume::Track).or(other),
        };
        old.engine.engine.shutdown();
        let mut config = config.unwrap_or(old.config);
        // the new engine plays at the same volume as the previous one
        config.initial_volume = volume;
        let host = self.clone();
        let credentials = old.credentials;
        rt.spawn(async move {
            let _ = host.connect(credentials, config, resume).await;
        });
    }

    /// The librespot session dropped on its own — reconnect (no more often than RECONNECT_LIMIT per 10 minutes).
    pub fn reconnect(self: &Arc<Self>, rt: &Handle, volume: u16) {
        let exhausted = {
            let mut reconnects = self.reconnects.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let now = Instant::now();
            reconnects.retain(|at| now.duration_since(*at) < RECONNECT_WINDOW);
            reconnects.push(now);
            reconnects.len() > RECONNECT_LIMIT
        };
        if exhausted {
            if let Some(old) = self.take() {
                old.engine.engine.shutdown();
            }
            let _ = self.tx.send(Msg::Local(LocalMsg::Failed(
                mf_i18n::t("Local playback keeps dropping. Turn it on again in Settings"),
            )));
            return;
        }
        tracing::info!("the librespot session ended; reconnecting");
        self.restart(rt, None, volume, None);
    }

    pub fn disconnect(&self) {
        self.next_generation();
        if let Some(old) = self.take() {
            old.engine.engine.shutdown();
        }
        let _ = self.tx.send(Msg::Local(LocalMsg::Unavailable));
    }

    /// Exit: stop without notifying the actor.
    pub fn shutdown(&self) {
        self.next_generation();
        if let Some(old) = self.take() {
            old.engine.engine.shutdown();
        }
    }
}

/// Pick up playback on a new session; Spirc may reject loading while it is registering.
fn resume_playback(link: Arc<EngineLink>, resume: PlaybackResume) {
    tokio::spawn(async move {
        for attempt in 0..3 {
            if let Err(error) = link.engine.resume(resume.clone()) {
                tracing::warn!("couldn't resume playback: {error:#}");
            }
            tokio::time::sleep(Duration::from_secs(4)).await;
            if link.engine.interrupted().is_some() {
                return;
            }
            tracing::info!("retrying playback resume (attempt {})", attempt + 2);
        }
    });
}
