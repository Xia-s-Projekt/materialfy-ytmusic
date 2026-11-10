//! List models for QML (owned by the "shell"). The bridges create them via DictListModel::create
//! and apply service changes via [`apply_list_change`].
pub mod list_model;
pub mod track_filter;

use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;
use md3_model::{ListChange, PlayContext, TrackItem};

pub use list_model::{ModelKind, Row, qobject::DictListModel};

/// An item (TrackItem, AlbumItem...) -> a model row.
pub fn to_row<T: Serialize>(item: &T) -> Row {
    match serde_json::to_value(item) {
        Ok(serde_json::Value::Object(map)) => Arc::new(map),
        _ => Arc::new(serde_json::Map::new()),
    }
}

pub fn to_rows<T: Serialize>(items: &[T]) -> Vec<Row> {
    items.iter().map(to_row).collect()
}

/// A change to the item list -> the same change in model rows (serialization - outside the GUI thread).
/// Order and repeated items are preserved as they are.
pub fn to_row_change<T: Serialize>(change: &ListChange<T>) -> ListChange<Row> {
    match change {
        ListChange::Reset(items) => ListChange::Reset(to_rows(items)),
        ListChange::Append(items) => ListChange::Append(to_rows(items)),
        ListChange::Insert { at, items } => ListChange::Insert { at: *at, items: to_rows(items) },
        ListChange::Remove { at, count } => ListChange::Remove { at: *at, count: *count },
        ListChange::Move { from, to } => ListChange::Move { from: *from, to: *to },
        ListChange::Loading(value) => ListChange::Loading(*value),
    }
}

/// Apply an already prepared change (in model rows) - without re-serializing.
pub fn apply_row_change(model: Pin<&mut DictListModel>, change: &ListChange<Row>) {
    match change {
        ListChange::Reset(rows) => model.reset_rows(rows.clone()),
        ListChange::Append(rows) => model.append_rows(rows.clone()),
        ListChange::Insert { at, items } => model.insert_rows(*at, items.clone()),
        ListChange::Remove { at, count } => model.remove_rows(*at, *count),
        ListChange::Move { from, to } => model.move_row(*from, *to),
        ListChange::Loading(value) => model.set_loading(*value),
    }
}

/// Apply a list change from sf-services to the model (in the GUI thread).
pub fn apply_list_change<T: Serialize>(model: Pin<&mut DictListModel>, change: &ListChange<T>) {
    apply_row_change(model, &to_row_change(change));
}

/// Tracks and context from the model that QML passed to Player.playFrom / shuffleFrom:
/// DictListModel (of kind Track) or TrackFilter (found rows + source context). Anything else -> None.
///
/// # Safety
/// `object` is a live QObject from QML (or null); call in the GUI thread.
pub unsafe fn tracks_from_model(object: *mut cxx_qt::QObject) -> Option<(Vec<TrackItem>, PlayContext)> {
    use cxx_qt::casting::Downcast;
    let object = unsafe { object.as_ref() }?;
    if let Some(model) = object.downcast::<DictListModel>() {
        let items = model
            .rows()
            .iter()
            .filter_map(|row| serde_json::from_value(serde_json::Value::Object((**row).clone())).ok())
            .collect();
        return Some((items, model.context().clone()));
    }
    track_filter::tracks_from_filter(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_change_keeps_order_and_repeats() {
        let track = |uri: &str| TrackItem { uri: uri.into(), ..Default::default() };
        let change =
            ListChange::Append(vec![track("spotify:track:1"), track("spotify:track:1"), track("spotify:track:2")]);
        let ListChange::Append(rows) = to_row_change(&change) else { panic!("Append stays Append") };
        assert_eq!(rows.len(), 3, "a repeated track is not collapsed");
        let uris: Vec<&str> = rows.iter().map(|r| r["uri"].as_str().unwrap_or_default()).collect();
        assert_eq!(uris, ["spotify:track:1", "spotify:track:1", "spotify:track:2"]);
    }
}
