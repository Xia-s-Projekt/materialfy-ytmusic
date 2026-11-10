//! TrackFilter { source: model; query: "text" } (trackfilter.py): search inside a track list.
//! Returns the same roles as the track model, and works for Player.playFrom: what was found goes into the queue.
//! An empty query (active = false) - all source tracks as they are. Search rules - md3_model::trackfilter.
//! Source changes (via rowsChanged) are replayed as a chunk or a reorder, so the list does not lose its scroll.
use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::CxxQtType;
use cxx_qt::casting::{Downcast, Upcast};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QString, QVariant};
use md3_model::trackfilter;
use md3_model::{PlayContext, TrackItem};

use super::list_model::{
    Row, TRACK_KEYS, find_in, qobject::DictListModel, role_names_for, row_data, row_map, total_duration,
};
use crate::qt::{QVariantMap, qvariant_to_json};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
        include!("cxx-qt-lib/qlist.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;
    }

    // Base - the type from the list_model bridge (re-declaring it in "C++Qt" duplicates the cast symbols)
    unsafe extern "C++" {
        include!("materialfy/src/models/list_model.cxxqt.h");
        type QAbstractListModel = crate::models::list_model::qobject::QAbstractListModel;
        include!(<QtCore/QObject>);
        type QObject = cxx_qt::QObject;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[base = QAbstractListModel]
        #[qproperty(*mut QObject, source, READ = source_object, WRITE = set_source, NOTIFY = source_changed)]
        #[qproperty(QString, query, READ, WRITE = set_query, NOTIFY = query_changed)]
        #[qproperty(bool, active, READ = active, NOTIFY = query_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        #[qproperty(bool, loading, READ = loading, NOTIFY = loading_changed)]
        #[qproperty(i64, total_duration_ms, cxx_name = "totalDurationMs", READ = total_duration_ms, NOTIFY = count_changed)]
        type TrackFilter = super::TrackFilterRust;

        #[cxx_name = "sourceObject"]
        fn source_object(self: &TrackFilter) -> *mut QObject;
        #[cxx_name = "setSource"]
        unsafe fn set_source(self: Pin<&mut TrackFilter>, source: *mut QObject);
        #[cxx_name = "setQuery"]
        fn set_query(self: Pin<&mut TrackFilter>, query: &QString);
        #[cxx_name = "isActive"]
        fn active(self: &TrackFilter) -> bool;
        fn count(self: &TrackFilter) -> i32;
        #[cxx_name = "isLoading"]
        fn loading(self: &TrackFilter) -> bool;
        #[cxx_name = "totalDurationMsValue"]
        fn total_duration_ms(self: &TrackFilter) -> i64;

        #[qsignal]
        #[cxx_name = "sourceChanged"]
        fn source_changed(self: Pin<&mut TrackFilter>);
        #[qsignal]
        #[cxx_name = "queryChanged"]
        fn query_changed(self: Pin<&mut TrackFilter>);
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut TrackFilter>);
        #[qsignal]
        #[cxx_name = "loadingChanged"]
        fn loading_changed(self: Pin<&mut TrackFilter>);

        #[qinvokable]
        fn get(self: &TrackFilter, row: i32) -> QMap_QString_QVariant;
        #[qinvokable]
        fn find(self: &TrackFilter, key: &QString, value: &QVariant) -> i32;

        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &TrackFilter, parent: &QModelIndex) -> i32;
        #[cxx_override]
        fn data(self: &TrackFilter, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &TrackFilter) -> QHash_i32_QByteArray;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut TrackFilter>);
        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut TrackFilter>);
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        fn begin_insert_rows(self: Pin<&mut TrackFilter>, parent: &QModelIndex, first: i32, last: i32);
        #[inherit]
        #[cxx_name = "endInsertRows"]
        fn end_insert_rows(self: Pin<&mut TrackFilter>);
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        fn begin_remove_rows(self: Pin<&mut TrackFilter>, parent: &QModelIndex, first: i32, last: i32);
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        fn end_remove_rows(self: Pin<&mut TrackFilter>);
        #[inherit]
        #[cxx_name = "beginMoveRows"]
        fn begin_move_rows(
            self: Pin<&mut TrackFilter>,
            source_parent: &QModelIndex,
            source_first: i32,
            source_last: i32,
            destination_parent: &QModelIndex,
            destination_child: i32,
        ) -> bool;
        #[inherit]
        #[cxx_name = "endMoveRows"]
        fn end_move_rows(self: Pin<&mut TrackFilter>);
        #[inherit]
        fn index(self: &TrackFilter, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;
        #[inherit]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut TrackFilter>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QList_i32,
        );
    }
}

cxx_qt::impl_transitive_cast!(qobject::TrackFilter, qobject::QAbstractListModel, cxx_qt::QObject);

pub struct TrackFilterRust {
    /// source (DictListModel lives until the process ends - the models are owned by singletons)
    model: *mut DictListModel,
    query: QString,
    words: Vec<String>,
    /// search rows over the source tracks, while it has not changed
    hay: Option<Vec<String>>,
    rows: Vec<Row>,
    /// subscriptions to the source's rowsChanged and loadingChanged; dropped when the source changes and the filter is deleted
    connection: Option<cxx_qt::QMetaObjectConnectionGuard>,
    loading_connection: Option<cxx_qt::QMetaObjectConnectionGuard>,
}

impl Default for TrackFilterRust {
    fn default() -> Self {
        Self {
            model: std::ptr::null_mut(),
            query: QString::default(),
            words: Vec::new(),
            hay: None,
            rows: Vec::new(),
            connection: None,
            loading_connection: None,
        }
    }
}

/// Pointer to the filter for the source signal handler (called in the GUI thread).
struct FilterPtr(*mut qobject::TrackFilter);

// SAFETY: the handler is called only in the GUI thread (AutoConnection, objects in one thread),
// and the connection is broken before the filter is deleted (a guard in its fields).
unsafe impl Send for FilterPtr {}

impl FilterPtr {
    fn get(&self) -> *mut qobject::TrackFilter {
        self.0
    }
}

impl qobject::TrackFilter {
    /// Source context (where the tracks come from) - for the player.
    pub fn context(&self) -> PlayContext {
        unsafe { self.rust().model.as_ref() }.map(|m| m.context().clone()).unwrap_or_default()
    }

    /// Found rows.
    pub fn rows(&self) -> &[Row] {
        &self.rust().rows
    }

    fn source_object(&self) -> *mut cxx_qt::QObject {
        let model = self.rust().model;
        if model.is_null() {
            return std::ptr::null_mut();
        }
        unsafe { <DictListModel as Upcast<cxx_qt::QObject>>::upcast_ptr(model).cast_mut() }
    }

    unsafe fn set_source(mut self: Pin<&mut Self>, source: *mut cxx_qt::QObject) {
        let model = unsafe { source.as_ref() }
            .and_then(|object| object.downcast::<DictListModel>())
            .map_or(std::ptr::null_mut(), |m| std::ptr::from_ref(m).cast_mut());
        if model == self.rust().model {
            return;
        }
        // old connections are broken when the guard is replaced
        let was_loading = self.loading();
        self.as_mut().rust_mut().connection = None;
        self.as_mut().rust_mut().loading_connection = None;
        self.as_mut().rust_mut().model = model;
        self.as_mut().rust_mut().hay = None;
        if let Some(source) = unsafe { model.as_mut() } {
            let this = FilterPtr(unsafe { self.as_mut().get_unchecked_mut() } as *mut Self);
            let source = unsafe { Pin::new_unchecked(source) };
            let connection = source.on_rows_changed(move |source| {
                // SAFETY: see FilterPtr
                let filter = unsafe { Pin::new_unchecked(&mut *this.get()) };
                filter.source_rows_changed(source.rows());
            });
            self.as_mut().rust_mut().connection = Some(connection);
            let this = FilterPtr(unsafe { self.as_mut().get_unchecked_mut() } as *mut Self);
            let source = unsafe { Pin::new_unchecked(&mut *model) };
            let connection = source.on_loading_changed(move |_| {
                // SAFETY: see FilterPtr
                unsafe { Pin::new_unchecked(&mut *this.get()) }.loading_changed();
            });
            self.as_mut().rust_mut().loading_connection = Some(connection);
        }
        self.as_mut().source_changed();
        if self.loading() != was_loading {
            self.as_mut().loading_changed();
        }
        let items = self.source_rows();
        self.refresh(items, true);
    }

    fn set_query(mut self: Pin<&mut Self>, query: &QString) {
        if *query == self.rust().query {
            return;
        }
        let words = trackfilter::query_words(&query.to_string());
        self.as_mut().rust_mut().query = query.clone();
        self.as_mut().rust_mut().words = words;
        self.as_mut().query_changed();
        let items = self.source_rows();
        self.refresh(items, true);
    }

    fn active(&self) -> bool {
        !self.rust().words.is_empty()
    }

    fn count(&self) -> i32 {
        self.rust().rows.len() as i32
    }

    /// source loading (page loading)
    fn loading(&self) -> bool {
        unsafe { self.rust().model.as_ref() }.is_some_and(|m| m.is_loading())
    }

    fn total_duration_ms(&self) -> i64 {
        total_duration(&self.rust().rows)
    }

    fn get(&self, row: i32) -> QVariantMap {
        row_map(&self.rust().rows, row)
    }

    fn find(&self, key: &QString, value: &QVariant) -> i32 {
        find_in(&self.rust().rows, &key.to_string(), &qvariant_to_json(value)).map_or(-1, |i| i as i32)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() { 0 } else { self.count() }
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        row_data(&self.rust().rows, TRACK_KEYS, index, role)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        role_names_for(TRACK_KEYS)
    }

    /// Source rows. The lifetime is not tied to self: the source is a separate object.
    fn source_rows<'a>(&self) -> &'a [Row] {
        match unsafe { self.rust().model.as_ref() } {
            Some(model) => {
                let rows: *const [Row] = model.rows();
                unsafe { &*rows }
            }
            None => &[],
        }
    }

    fn source_rows_changed(mut self: Pin<&mut Self>, items: &[Row]) {
        self.as_mut().rust_mut().hay = None;
        self.refresh(items, false);
    }

    /// _refresh from trackfilter.py.
    fn refresh(mut self: Pin<&mut Self>, items: &[Row], reset: bool) {
        let found: Vec<Row> = if self.rust().words.is_empty() {
            items.to_vec()
        } else {
            let hay = self.as_mut().rust_mut().hay.take();
            let hay = hay.unwrap_or_else(|| items.iter().map(|row| trackfilter::haystack(row)).collect());
            let words = &self.rust().words;
            let found = items
                .iter()
                .zip(&hay)
                .filter(|(_, h)| trackfilter::matches(h, words))
                .map(|(row, _)| row.clone())
                .collect();
            self.as_mut().rust_mut().hay = Some(hay);
            found
        };
        if reset {
            self.reset_rows(found);
            return;
        }
        // The source changed. Usually it is a single chunk - page loading, a new like at the start, a removed like:
        // we replay the same change.
        let old = &self.rust().rows;
        let common = found.len().min(old.len());
        let mut head = 0;
        while head < common && Arc::ptr_eq(&found[head], &old[head]) {
            head += 1;
        }
        let mut tail = 0;
        while tail < common - head && Arc::ptr_eq(&found[found.len() - 1 - tail], &old[old.len() - 1 - tail]) {
            tail += 1;
        }
        let old_len = old.len();
        if head + tail == old_len && found.len() > old_len {
            let inserted = found[head..found.len() - tail].to_vec();
            self.insert_rows(head, inserted);
        } else if head + tail == found.len() && old_len > found.len() {
            self.remove_rows(head, old_len - found.len());
        } else if found.len() == old_len && head < old_len {
            // the same length: a single row reorder (moveTracks) or replacing rows in place
            let last = old_len - tail - 1;
            let (a, b) = (&old[head..=last], &found[head..=last]);
            let ptr_eq = |x: &[Row], y: &[Row]| x.iter().zip(y).all(|(p, q)| Arc::ptr_eq(p, q));
            let n = a.len();
            if n >= 2 && Arc::ptr_eq(&a[0], &b[n - 1]) && ptr_eq(&a[1..], &b[..n - 1]) {
                self.move_row(head, last, found);
            } else if n >= 2 && Arc::ptr_eq(&a[n - 1], &b[0]) && ptr_eq(&a[..n - 1], &b[1..]) {
                self.move_row(last, head, found);
            } else {
                self.as_mut().rust_mut().rows = found;
                let parent = QModelIndex::default();
                let top = self.index(head as i32, 0, &parent);
                let bottom = self.index(last as i32, 0, &parent);
                self.as_mut().data_changed(&top, &bottom, &QList::default());
            }
        } else if found.len() != old_len || head != old_len {
            self.reset_rows(found);
        }
    }

    /// Row from -> to; `rows` are the resulting rows (already reordered).
    fn move_row(mut self: Pin<&mut Self>, from: usize, to: usize, rows: Vec<Row>) {
        let child = if to > from { to + 1 } else { to };
        let parent = QModelIndex::default();
        if self.as_mut().begin_move_rows(&parent, from as i32, from as i32, &parent, child as i32) {
            self.as_mut().rust_mut().rows = rows;
            self.as_mut().end_move_rows();
        } else {
            self.reset_rows(rows);
        }
    }

    fn reset_rows(mut self: Pin<&mut Self>, rows: Vec<Row>) {
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();
        self.count_changed();
    }

    fn insert_rows(mut self: Pin<&mut Self>, at: usize, rows: Vec<Row>) {
        if rows.is_empty() {
            return;
        }
        let last = at + rows.len() - 1;
        self.as_mut().begin_insert_rows(&QModelIndex::default(), at as i32, last as i32);
        self.as_mut().rust_mut().rows.splice(at..at, rows);
        self.as_mut().end_insert_rows();
        self.count_changed();
    }

    fn remove_rows(mut self: Pin<&mut Self>, at: usize, count: usize) {
        if count == 0 {
            return;
        }
        self.as_mut().begin_remove_rows(&QModelIndex::default(), at as i32, (at + count - 1) as i32);
        self.as_mut().rust_mut().rows.drain(at..at + count);
        self.as_mut().end_remove_rows();
        self.count_changed();
    }
}

/// Found rows and the source context, if `object` is a TrackFilter.
pub fn tracks_from_filter(object: &cxx_qt::QObject) -> Option<(Vec<TrackItem>, PlayContext)> {
    let filter = object.downcast::<qobject::TrackFilter>()?;
    let items = filter
        .rows()
        .iter()
        .filter_map(|row| serde_json::from_value(serde_json::Value::Object((**row).clone())).ok())
        .collect();
    let mut context = filter.context();
    // the found rows are not the whole context: play as a URI list (queue.md), the source type stays for Player.source
    if filter.active() {
        context.context_uri = None;
    }
    Some((items, context))
}
