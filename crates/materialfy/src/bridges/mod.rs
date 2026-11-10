//! Materialfy.Core QML singletons. Every file with #[cxx_qt::bridge]
//! is picked up by build.rs automatically; this file holds the module declarations and shared bridge helpers.
pub mod auth;
pub mod catalog;
pub mod covers;
pub mod crash_report;
pub mod data;
pub mod devices;
pub mod home;
pub mod library;
pub mod lyrics;
pub mod player;
pub mod radio;
pub mod releases;
pub mod settings;
pub mod theme;
pub mod translations;
pub mod updater;

use std::pin::Pin;
use std::time::{Duration, Instant};

use cxx::UniquePtr;
use cxx_qt::casting::Upcast;
use cxx_qt::{CxxQtThread, Threading};
use serde_json::Value;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::watch;
use md3_model::TrackItem;

use crate::context::ctx;
use crate::models::DictListModel;
use crate::qt::{QVariantMap, qvariantmap_to_json};

/// What arrived from the service's broadcast channel.
pub(crate) enum Delivery<E> {
    Event(E),
    /// events were missed — the bridge re-reads the entire service state
    Lagged,
}

/// Service event listener on tokio: every event is moved into the GUI thread.
/// The channel is closed or the object is destroyed — the task finishes.
pub(crate) fn forward_events<T, E>(
    thread: CxxQtThread<T>,
    mut events: broadcast::Receiver<E>,
    apply: fn(Pin<&mut T>, Delivery<E>),
) where
    T: Threading + 'static,
    E: Clone + Send + 'static,
{
    ctx().rt.spawn(async move {
        loop {
            let delivery = match events.recv().await {
                Ok(event) => Delivery::Event(event),
                Err(RecvError::Lagged(_)) => Delivery::Lagged,
                Err(RecvError::Closed) => break,
            };
            if thread.queue(move |object| apply(object, delivery)).is_err() {
                break;
            }
        }
    });
}

/// Snapshot (watch) listener: every change becomes a snapshot in the GUI thread.
pub(crate) fn forward_watch<T, S>(thread: CxxQtThread<T>, mut snapshot: watch::Receiver<S>, apply: fn(Pin<&mut T>, S))
where
    T: Threading + 'static,
    S: Clone + Send + Sync + 'static,
{
    ctx().rt.spawn(async move {
        while snapshot.changed().await.is_ok() {
            let value = snapshot.borrow_and_update().clone();
            if thread.queue(move |object| apply(object, value)).is_err() {
                break;
            }
        }
    });
}

/// Model owned by the bridge → a pointer for QML (a CONSTANT property or an invokable return value).
pub(crate) fn model_ptr(model: &UniquePtr<DictListModel>) -> *mut cxx_qt::QObject {
    model.as_ref().map_or(std::ptr::null_mut(), |m| unsafe {
        <DictListModel as Upcast<cxx_qt::QObject>>::upcast_ptr(m).cast_mut()
    })
}

/// Apply a change to the model, if there is one.
pub(crate) fn with_model(model: &mut UniquePtr<DictListModel>, f: impl FnOnce(Pin<&mut DictListModel>)) {
    if let Some(m) = model.as_mut() {
        f(m);
    }
}

/// How long to hold a locally written value without accepting an "echo" from the service (it may be older than the write).
pub(crate) const ECHO_HOLD: Duration = Duration::from_millis(500);

pub(crate) fn held(since: Option<Instant>) -> bool {
    since.is_some_and(|at| at.elapsed() < ECHO_HOLD)
}

/// Dictionary from QML → JSON with numbers normalized as in Python (5.0 → 5; a numeric id → a string).
pub(crate) fn map_to_json(map: &QVariantMap) -> serde_json::Map<String, Value> {
    let mut object = qvariantmap_to_json(map);
    for (key, value) in object.iter_mut() {
        normalize(key, value);
    }
    object
}

fn normalize(key: &str, value: &mut Value) {
    match value {
        Value::Number(n) if matches!(key, "trackId" | "albumId" | "artistId" | "id") => {
            let text = match n.as_i64() {
                Some(i) => i.to_string(),
                None => n.as_f64().map(|f| if f.fract() == 0.0 { (f as i64).to_string() } else { f.to_string() })
                    .unwrap_or_default(),
            };
            *value = Value::String(text);
        }
        Value::Number(n) => {
            if n.as_i64().is_none() && n.as_u64().is_none()
                && let Some(f) = n.as_f64()
                && f.fract() == 0.0
                && f.abs() < 9.0e15
            {
                *value = Value::from(f as i64);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| normalize("", item)),
        Value::Object(map) => map.iter_mut().for_each(|(k, v)| normalize(k, v)),
        _ => {}
    }
}

/// Track dictionary from QML → TrackItem (a partial dictionary gives default fields; without trackId and uri — None).
pub(crate) fn track_from_map(map: &QVariantMap) -> Option<TrackItem> {
    track_from_json(map_to_json(map))
}

/// Track dictionary parsing: without the available key the track counts as available; an episode is also recognized by its URI.
fn track_from_json(mut object: serde_json::Map<String, Value>) -> Option<TrackItem> {
    object.entry("available").or_insert(Value::Bool(true));
    // strings in place of numbers (durationMs: "1000") must not break parsing of the whole dictionary
    for key in ["durationMs"] {
        if let Some(Value::String(text)) = object.get(key) {
            let number = text.trim().parse::<i64>().unwrap_or(0);
            object.insert(key.to_owned(), Value::from(number));
        }
    }
    let mut track: TrackItem = serde_json::from_value(Value::Object(object)).ok()?;
    if track.uri.starts_with("spotify:episode:") {
        track.is_episode = true;
    }
    (!track.track_id.is_empty() || !track.uri.is_empty()).then_some(track)
}

/// QStringList → Vec<String>.
pub(crate) fn string_list(list: &cxx_qt_lib::QStringList) -> Vec<String> {
    let list: cxx_qt_lib::QList<cxx_qt_lib::QString> = list.into();
    list.iter().map(|s| s.to_string()).collect()
}

/// Serializable → QVariantMap ({} on error).
pub(crate) fn to_qmap<T: serde::Serialize>(value: &T) -> QVariantMap {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => crate::qt::json_to_qvariantmap(&map),
        _ => QVariantMap::default(),
    }
}

/// Serializable list → QVariantList.
pub(crate) fn to_qlist<T: serde::Serialize>(items: &[T]) -> crate::qt::QVariantList {
    match serde_json::to_value(items) {
        Ok(Value::Array(items)) => crate::qt::json_to_qvariantlist(&items),
        _ => crate::qt::QVariantList::default(),
    }
}

/// file:// URL → local path, otherwise the string unchanged (QUrl.isLocalFile in Python).
pub(crate) fn local_path(source: &cxx_qt_lib::QString) -> String {
    let url = cxx_qt_lib::QUrl::from(source);
    if url.is_local_file() { url.to_local_file_or_default().to_string() } else { source.to_string() }
}

/// String value of a dictionary key (a number — as text).
pub(crate) fn json_str(object: &serde_json::Map<String, Value>, key: &str) -> String {
    match object.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(value: Value) -> serde_json::Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn ids_become_strings_and_whole_floats_ints() {
        let mut value = serde_json::json!({"trackId": 42.0, "durationMs": 1000.0, "artistRefs": [{"id": 7, "name": "A"}]});
        if let Value::Object(map) = &mut value {
            map.iter_mut().for_each(|(k, v)| normalize(k, v));
        }
        assert_eq!(value["trackId"], "42");
        assert_eq!(value["durationMs"], 1000);
        assert_eq!(value["artistRefs"][0]["id"], "7");
    }

    #[test]
    fn track_needs_id_or_uri() {
        assert!(track_from_json(object(serde_json::json!({"title": "x"}))).is_none());
        let track = track_from_json(object(serde_json::json!({"uri": "spotify:track:abc"}))).unwrap();
        assert!(track.available);
        assert!(!track.is_episode);
        let episode = track_from_json(object(serde_json::json!({"uri": "spotify:episode:e1", "durationMs": "5"}))).unwrap();
        assert!(episode.is_episode);
        assert_eq!(episode.duration_ms, 5);
        let hidden = track_from_json(object(serde_json::json!({"trackId": "t", "available": false}))).unwrap();
        assert!(!hidden.available);
    }
}
