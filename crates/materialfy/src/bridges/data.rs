// AlbumData / ArtistData / ShowData / EpisodeData — данные страниц, которые возвращают Catalog.album/artist/show/episode(id).
// Создаются из Rust (make_unique) с CppOwnership и живут в кэше моста Catalog до конца процесса.
use std::pin::Pin;

use std::collections::HashSet;

use cxx::UniquePtr;
use cxx_qt::casting::Upcast;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde::Serialize;
use md3_model::{EpisodeItem, PlayContext, contexts};
use yt_api::YtError;
use yt_services::{AlbumPage, ArtistPage, EpisodePage, ServiceError, ShowPage};

use super::{model_ptr, with_model};
use crate::context::ctx;
use crate::models::{DictListModel, ModelKind, to_rows};
use crate::qt::{QVariantMap, json_to_qvariantmap};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(QMap_QString_QVariant, info, READ, NOTIFY = changed)]
        #[qproperty(bool, loading, READ, NOTIFY = changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = changed)]
        #[qproperty(*mut QObject, tracks, READ = tracks_ptr, CONSTANT)]
        type AlbumData = super::AlbumDataRust;

        fn tracks_ptr(self: &AlbumData) -> *mut QObject;

        #[qsignal]
        fn changed(self: Pin<&mut AlbumData>);

        #[qobject]
        #[qproperty(QMap_QString_QVariant, info, READ, NOTIFY = changed)]
        #[qproperty(bool, loading, READ, NOTIFY = changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = changed)]
        #[qproperty(*mut QObject, popular, READ = popular_ptr, CONSTANT)]
        #[qproperty(*mut QObject, albums, READ = albums_ptr, CONSTANT)]
        #[qproperty(*mut QObject, also_albums, cxx_name = "alsoAlbums", READ = also_albums_ptr, CONSTANT)]
        #[qproperty(*mut QObject, similar, READ = similar_ptr, CONSTANT)]
        type ArtistData = super::ArtistDataRust;

        fn popular_ptr(self: &ArtistData) -> *mut QObject;
        fn albums_ptr(self: &ArtistData) -> *mut QObject;
        fn also_albums_ptr(self: &ArtistData) -> *mut QObject;
        fn similar_ptr(self: &ArtistData) -> *mut QObject;

        #[qsignal]
        fn changed(self: Pin<&mut ArtistData>);

        #[qobject]
        #[qproperty(QMap_QString_QVariant, info, READ, NOTIFY = changed)]
        #[qproperty(bool, loading, READ, NOTIFY = changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = changed)]
        #[qproperty(i32, total, READ, NOTIFY = changed)]
        /// идёт догрузка следующей страницы (loadMore)
        #[qproperty(bool, loading_more, cxx_name = "loadingMore", READ, NOTIFY = changed)]
        #[qproperty(*mut QObject, episodes, READ = episodes_ptr, CONSTANT)]
        type ShowData = super::ShowDataRust;

        fn episodes_ptr(self: &ShowData) -> *mut QObject;

        #[qsignal]
        fn changed(self: Pin<&mut ShowData>);
        /// Догрузить следующую страницу выпусков (по 50).
        #[qinvokable]
        #[cxx_name = "loadMore"]
        fn load_more(self: Pin<&mut ShowData>);

        #[qobject]
        #[qproperty(QMap_QString_QVariant, info, READ, NOTIFY = changed)]
        #[qproperty(QMap_QString_QVariant, show, READ, NOTIFY = changed)]
        #[qproperty(bool, loading, READ, NOTIFY = changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = changed)]
        #[qproperty(*mut QObject, more, READ = more_ptr, CONSTANT)]
        type EpisodeData = super::EpisodeDataRust;

        fn more_ptr(self: &EpisodeData) -> *mut QObject;

        #[qsignal]
        fn changed(self: Pin<&mut EpisodeData>);
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/common.h");
        #[doc(hidden)]
        #[namespace = "rust::cxxqtlib1"]
        #[cxx_name = "make_unique"]
        fn album_data_make_unique() -> UniquePtr<AlbumData>;
        #[doc(hidden)]
        #[namespace = "rust::cxxqtlib1"]
        #[cxx_name = "make_unique"]
        fn artist_data_make_unique() -> UniquePtr<ArtistData>;
        #[doc(hidden)]
        #[namespace = "rust::cxxqtlib1"]
        #[cxx_name = "make_unique"]
        fn show_data_make_unique() -> UniquePtr<ShowData>;
        #[doc(hidden)]
        #[namespace = "rust::cxxqtlib1"]
        #[cxx_name = "make_unique"]
        fn episode_data_make_unique() -> UniquePtr<EpisodeData>;
    }

    impl cxx_qt::Threading for ShowData {}
}

/// Страница каталога: объект во владении Rust, который Catalog создаёт, кэширует и заполняет.
pub(crate) trait PageData: cxx::memory::UniquePtrTarget + Sized + 'static {
    type Page: Send + 'static;
    fn new_object() -> UniquePtr<Self>;
    fn object_ptr(self: Pin<&mut Self>) -> *mut cxx_qt::QObject;
    fn failed(&self) -> bool;
    fn reload(self: Pin<&mut Self>);
    fn fill(self: Pin<&mut Self>, page: Self::Page);
    fn fail(self: Pin<&mut Self>, text: &str);
}

macro_rules! page_data {
    ($ty:ty, $page:ty) => {
        impl PageData for $ty {
            type Page = $page;
            fn new_object() -> UniquePtr<Self> {
                <$ty>::create()
            }
            fn object_ptr(self: Pin<&mut Self>) -> *mut cxx_qt::QObject {
                self.as_qobject_ptr()
            }
            fn failed(&self) -> bool {
                self.has_error()
            }
            fn reload(self: Pin<&mut Self>) {
                self.begin_loading()
            }
            fn fill(self: Pin<&mut Self>, page: $page) {
                self.set_page(page)
            }
            fn fail(self: Pin<&mut Self>, text: &str) {
                self.set_error(text)
            }
        }
    };
}

page_data!(qobject::AlbumData, AlbumPage);
page_data!(qobject::ArtistData, ArtistPage);
page_data!(qobject::ShowData, ShowPage);
page_data!(qobject::EpisodeData, EpisodePage);

/// Текст ошибки загрузки страницы — по-русски.
pub(crate) fn error_text(error: &ServiceError) -> String {
    match error {
        ServiceError::NotSignedIn | ServiceError::Api(YtError::NotSignedIn) => mf_i18n::t("Sign in to your account"),
        ServiceError::Api(YtError::PythonMissing) => mf_i18n::t("Install ytmusicapi (pip install ytmusicapi)"),
        ServiceError::Api(YtError::Remote(message)) if message.contains("404") => mf_i18n::t("Not found on YouTube Music"),
        ServiceError::Api(YtError::Network(_)) => mf_i18n::t("No connection to YouTube Music"),
        ServiceError::Api(_) => mf_i18n::t("Couldn't load"),
        ServiceError::Other(text) if !text.is_empty() => text.clone(),
        ServiceError::Other(_) => mf_i18n::t("Couldn't load"),
    }
}

fn to_map<T: Serialize>(value: &T) -> QVariantMap {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::Object(map)) => json_to_qvariantmap(&map),
        _ => QVariantMap::default(),
    }
}

/// Контекст модели страницы: тип + URI, название и обложка для карточки «Играет из …».
fn context(kind: &str, uri: &str, title: &str, cover: &str) -> PlayContext {
    if uri.is_empty() {
        PlayContext::new(kind)
    } else {
        PlayContext::with_uri(kind, uri).titled(title).with_cover(cover)
    }
}

pub struct AlbumDataRust {
    info: QVariantMap,
    loading: bool,
    error_text: QString,
    tracks: UniquePtr<DictListModel>,
}

impl Default for AlbumDataRust {
    fn default() -> Self {
        let mut tracks = DictListModel::create(ModelKind::Track);
        with_model(&mut tracks, |m| m.set_loading(true));
        Self { info: QVariantMap::default(), loading: true, error_text: QString::default(), tracks }
    }
}

impl qobject::AlbumData {
    /// Новый объект во владении Rust (QML его не удалит).
    pub fn create() -> UniquePtr<Self> {
        let data = qobject::album_data_make_unique();
        if !data.is_null() {
            unsafe { crate::qt::set_cpp_ownership(<Self as Upcast<cxx_qt::QObject>>::upcast_ptr(data.as_mut_ptr()).cast_mut()) };
        }
        data
    }

    pub fn as_qobject_ptr(self: Pin<&mut Self>) -> *mut cxx_qt::QObject {
        let this: *const Self = &*self;
        unsafe { <Self as Upcast<cxx_qt::QObject>>::upcast_ptr(this).cast_mut() }
    }

    pub fn has_error(&self) -> bool {
        !self.error_text.is_empty()
    }

    fn tracks_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.tracks)
    }

    /// Повторная загрузка после ошибки.
    pub fn begin_loading(mut self: Pin<&mut Self>) {
        let mut this = self.as_mut().rust_mut();
        this.loading = true;
        this.error_text = QString::default();
        with_model(&mut this.tracks, |m| m.set_loading(true));
        self.changed();
    }

    pub fn set_page(mut self: Pin<&mut Self>, page: AlbumPage) {
        let mut this = self.as_mut().rust_mut();
        this.info = to_map(&page.info);
        with_model(&mut this.tracks, |mut m| {
            m.as_mut().set_context(context(contexts::ALBUM, &page.info.item.uri, &page.info.item.title, &page.info.item.cover));
            m.as_mut().reset_rows(to_rows(&page.tracks));
            m.set_loading(false);
        });
        this.loading = false;
        this.error_text = QString::default();
        self.changed();
    }

    pub fn set_error(mut self: Pin<&mut Self>, text: &str) {
        let mut this = self.as_mut().rust_mut();
        with_model(&mut this.tracks, |m| m.set_loading(false));
        this.loading = false;
        this.error_text = QString::from(text);
        self.changed();
    }
}

pub struct ArtistDataRust {
    info: QVariantMap,
    loading: bool,
    error_text: QString,
    popular: UniquePtr<DictListModel>,
    albums: UniquePtr<DictListModel>,
    also_albums: UniquePtr<DictListModel>,
    similar: UniquePtr<DictListModel>,
}

impl Default for ArtistDataRust {
    fn default() -> Self {
        Self {
            info: QVariantMap::default(),
            loading: true,
            error_text: QString::default(),
            popular: DictListModel::create(ModelKind::Track),
            albums: DictListModel::create(ModelKind::Album),
            also_albums: DictListModel::create(ModelKind::Album),
            similar: DictListModel::create(ModelKind::Artist),
        }
    }
}

impl qobject::ArtistData {
    pub fn create() -> UniquePtr<Self> {
        let data = qobject::artist_data_make_unique();
        if !data.is_null() {
            unsafe { crate::qt::set_cpp_ownership(<Self as Upcast<cxx_qt::QObject>>::upcast_ptr(data.as_mut_ptr()).cast_mut()) };
        }
        data
    }

    pub fn as_qobject_ptr(self: Pin<&mut Self>) -> *mut cxx_qt::QObject {
        let this: *const Self = &*self;
        unsafe { <Self as Upcast<cxx_qt::QObject>>::upcast_ptr(this).cast_mut() }
    }

    pub fn has_error(&self) -> bool {
        !self.error_text.is_empty()
    }

    fn popular_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.popular)
    }

    fn albums_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.albums)
    }

    fn also_albums_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.also_albums)
    }

    fn similar_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.similar)
    }

    pub fn begin_loading(mut self: Pin<&mut Self>) {
        let mut this = self.as_mut().rust_mut();
        this.loading = true;
        this.error_text = QString::default();
        self.changed();
    }

    pub fn set_page(mut self: Pin<&mut Self>, page: ArtistPage) {
        let mut this = self.as_mut().rust_mut();
        this.info = to_map(&page.info);
        with_model(&mut this.popular, |mut m| {
            m.as_mut().set_context(context(contexts::ARTIST, &page.info.item.uri, &page.info.item.name, &page.info.item.cover));
            m.reset_rows(to_rows(&page.popular));
        });
        with_model(&mut this.albums, |m| m.reset_rows(to_rows(&page.albums)));
        with_model(&mut this.also_albums, |m| m.reset_rows(to_rows(&page.also_albums)));
        with_model(&mut this.similar, |m| m.reset_rows(to_rows(&page.similar)));
        this.loading = false;
        this.error_text = QString::default();
        self.changed();
    }

    pub fn set_error(mut self: Pin<&mut Self>, text: &str) {
        let mut this = self.as_mut().rust_mut();
        this.loading = false;
        this.error_text = QString::from(text);
        self.changed();
    }
}

pub struct ShowDataRust {
    info: QVariantMap,
    loading: bool,
    error_text: QString,
    total: i32,
    episodes: UniquePtr<DictListModel>,
    loading_more: bool,
    /// id шоу (для loadMore)
    show_id: String,
    /// номер заполнения страницы: ответ loadMore от прошлого заполнения отбрасывается
    fills: u64,
}

impl Default for ShowDataRust {
    fn default() -> Self {
        let mut episodes = DictListModel::create(ModelKind::Episode);
        with_model(&mut episodes, |m| m.set_loading(true));
        Self {
            info: QVariantMap::default(),
            loading: true,
            error_text: QString::default(),
            total: 0,
            loading_more: false,
            episodes,
            show_id: String::new(),
            fills: 0,
        }
    }
}

impl qobject::ShowData {
    pub fn create() -> UniquePtr<Self> {
        let data = qobject::show_data_make_unique();
        if !data.is_null() {
            unsafe { crate::qt::set_cpp_ownership(<Self as Upcast<cxx_qt::QObject>>::upcast_ptr(data.as_mut_ptr()).cast_mut()) };
        }
        data
    }

    pub fn as_qobject_ptr(self: Pin<&mut Self>) -> *mut cxx_qt::QObject {
        let this: *const Self = &*self;
        unsafe { <Self as Upcast<cxx_qt::QObject>>::upcast_ptr(this).cast_mut() }
    }

    pub fn has_error(&self) -> bool {
        !self.error_text.is_empty()
    }

    fn episodes_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.episodes)
    }

    fn loaded_count(&self) -> usize {
        self.episodes.as_ref().map_or(0, |m| m.rows().len())
    }

    /// Следующая страница выпусков; повторный вызов во время догрузки и после последней страницы — ничего.
    fn load_more(mut self: Pin<&mut Self>) {
        let offset = self.loaded_count();
        if self.loading || self.loading_more || self.show_id.is_empty() || offset >= self.total.max(0) as usize {
            return;
        }
        let thread = self.qt_thread();
        let (show_id, fills) = (self.show_id.clone(), self.fills);
        self.as_mut().rust_mut().loading_more = true;
        self.as_mut().changed();
        let catalog = ctx().services.catalog.clone();
        ctx().rt.spawn(async move {
            let result = catalog.show_episodes(&show_id, offset as u32).await;
            let _ = thread.queue(move |data| data.more_loaded(fills, offset, result));
        });
    }

    fn more_loaded(mut self: Pin<&mut Self>, fills: u64, offset: usize, result: Result<Vec<EpisodeItem>, ServiceError>) {
        if fills != self.fills || offset != self.loaded_count() {
            return;
        }
        let mut this = self.as_mut().rust_mut();
        this.loading_more = false;
        match result {
            Ok(items) => {
                let known: HashSet<String> = this
                    .episodes
                    .as_ref()
                    .map(|m| m.rows().iter().map(|row| super::json_str(row, "episodeId")).collect())
                    .unwrap_or_default();
                let fresh: Vec<EpisodeItem> = items.into_iter().filter(|e| !known.contains(&e.episode_id)).collect();
                if fresh.is_empty() {
                    // Spotify отдал меньше, чем обещал total: больше не просить
                    this.total = offset.min(i32::MAX as usize) as i32;
                } else {
                    with_model(&mut this.episodes, |m| m.append_rows(to_rows(&fresh)));
                }
            }
            Err(error) => tracing::warn!("show episodes: {error}"),
        }
        self.changed();
    }

    pub fn begin_loading(mut self: Pin<&mut Self>) {
        let mut this = self.as_mut().rust_mut();
        this.loading = true;
        this.error_text = QString::default();
        self.changed();
    }

    pub fn set_page(mut self: Pin<&mut Self>, page: ShowPage) {
        let mut this = self.as_mut().rust_mut();
        this.info = to_map(&page.info);
        this.total = page.total.clamp(0, i32::MAX as i64) as i32;
        if !page.info.show_id.is_empty() {
            this.show_id = page.info.show_id.clone();
        }
        this.fills += 1;
        this.loading_more = false;
        with_model(&mut this.episodes, |mut m| {
            m.as_mut().set_context(context(contexts::SHOW, &page.info.uri, &page.info.name, &page.info.cover));
            m.as_mut().reset_rows(to_rows(&page.episodes));
            m.set_loading(false);
        });
        this.loading = false;
        this.error_text = QString::default();
        self.changed();
    }

    /// id шоу до загрузки (если в ответе его не окажется).
    pub fn set_show_id(self: Pin<&mut Self>, id: &str) {
        self.rust_mut().show_id = id.to_owned();
    }

    pub fn set_error(mut self: Pin<&mut Self>, text: &str) {
        let mut this = self.as_mut().rust_mut();
        with_model(&mut this.episodes, |m| m.set_loading(false));
        this.fills += 1;
        this.loading_more = false;
        this.loading = false;
        this.error_text = QString::from(text);
        self.changed();
    }
}

pub struct EpisodeDataRust {
    info: QVariantMap,
    show: QVariantMap,
    loading: bool,
    error_text: QString,
    more: UniquePtr<DictListModel>,
}

impl Default for EpisodeDataRust {
    fn default() -> Self {
        Self {
            info: QVariantMap::default(),
            show: QVariantMap::default(),
            loading: true,
            error_text: QString::default(),
            more: DictListModel::create(ModelKind::Episode),
        }
    }
}

impl qobject::EpisodeData {
    pub fn create() -> UniquePtr<Self> {
        let data = qobject::episode_data_make_unique();
        if !data.is_null() {
            unsafe { crate::qt::set_cpp_ownership(<Self as Upcast<cxx_qt::QObject>>::upcast_ptr(data.as_mut_ptr()).cast_mut()) };
        }
        data
    }

    pub fn as_qobject_ptr(self: Pin<&mut Self>) -> *mut cxx_qt::QObject {
        let this: *const Self = &*self;
        unsafe { <Self as Upcast<cxx_qt::QObject>>::upcast_ptr(this).cast_mut() }
    }

    pub fn has_error(&self) -> bool {
        !self.error_text.is_empty()
    }

    fn more_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.more)
    }

    pub fn begin_loading(mut self: Pin<&mut Self>) {
        let mut this = self.as_mut().rust_mut();
        this.loading = true;
        this.error_text = QString::default();
        self.changed();
    }

    pub fn set_page(mut self: Pin<&mut Self>, page: EpisodePage) {
        let mut this = self.as_mut().rust_mut();
        this.info = to_map(&page.info);
        this.show = to_map(&page.show);
        with_model(&mut this.more, |mut m| {
            m.as_mut().set_context(context(contexts::SHOW, &page.show.uri, &page.show.name, &page.show.cover));
            m.reset_rows(to_rows(&page.more));
        });
        this.loading = false;
        this.error_text = QString::default();
        self.changed();
    }

    pub fn set_error(mut self: Pin<&mut Self>, text: &str) {
        let mut this = self.as_mut().rust_mut();
        this.loading = false;
        this.error_text = QString::from(text);
        self.changed();
    }
}
