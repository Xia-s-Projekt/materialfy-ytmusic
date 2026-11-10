//! Covers: a fallback cover not from Spotify (yt_services::cover_fallback, Deezer). MorphImage asks for it when
//! the Spotify image could not be loaded from any of its CDNs: find(kind, artist, title) returns a request key,
//! the response comes via the found(key, url) signal - url is empty if nothing was found.
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use cxx_qt::Threading;
use cxx_qt_lib::QString;
use yt_services::cover_fallback::{CoverFallback, CoverKind};

use crate::context::ctx;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        type Covers = super::CoversRust;

        /// Response to find: the cover url or an empty string
        #[qsignal]
        fn found(self: Pin<&mut Covers>, key: QString, url: QString);

        /// kind: "album" | "track" | "artist"; artist - the first artist; title - the name (empty for artist).
        /// Returns a request key (empty - there is nothing to search for).
        #[qinvokable]
        fn find(self: Pin<&mut Covers>, kind: &QString, artist: &QString, title: &QString) -> QString;
    }

    impl cxx_qt::Threading for Covers {}
}

#[derive(Default)]
pub struct CoversRust;

fn fallback() -> &'static Arc<CoverFallback> {
    static FALLBACK: OnceLock<Arc<CoverFallback>> = OnceLock::new();
    FALLBACK.get_or_init(CoverFallback::new)
}

impl qobject::Covers {
    fn find(self: Pin<&mut Self>, kind: &QString, artist: &QString, title: &QString) -> QString {
        let Some(cover_kind) = CoverKind::parse(&kind.to_string()) else {
            return QString::default();
        };
        let (artist, title) = (artist.to_string(), title.to_string());
        if artist.trim().is_empty() {
            return QString::default();
        }
        let key = format!("{kind}\u{1f}{artist}\u{1f}{title}");
        let thread = self.qt_thread();
        let http = ctx().services.api.http();
        let answer_key = key.clone();
        ctx().rt.spawn(async move {
            let url = fallback().find(&http, cover_kind, &artist, &title).await.unwrap_or_default();
            if !url.is_empty() {
                tracing::debug!("fallback cover for {artist} — {title}");
            }
            let _ = thread.queue(move |covers| covers.found(QString::from(answer_key.as_str()), QString::from(url.as_str())));
        });
        QString::from(key.as_str())
    }
}
