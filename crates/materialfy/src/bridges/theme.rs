// ThemeEngine: MD3 color roles from a seed color or cover (PlayerSnapshot.cover_file). Recalculation - in spawn_blocking with a number
// of the request: a stale result is discarded, a cover that has not been parsed yet is not lost.
use std::path::PathBuf;
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QColor, QString, QStringList, QVariant};
use mf_platform::{SettingKey, SettingValue, SettingsStore};
use yt_player::PlayerEvent;
use mf_theme::{Scheme, Variant};

use super::{Delivery, forward_events, local_path};
use crate::context::ctx;
use crate::qt::QVariantMap;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qcolor.h");
        type QColor = cxx_qt_lib::QColor;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(QMap_QString_QVariant, colors, READ, NOTIFY = scheme_changed)]
        #[qproperty(QColor, seed, READ = seed_color, NOTIFY = seed_changed)]
        #[qproperty(bool, dark, READ, WRITE = set_dark, NOTIFY = dark_changed)]
        #[qproperty(QString, variant, READ = variant_name, WRITE = set_variant, NOTIFY = variant_changed)]
        #[qproperty(QStringList, variants, READ = variant_names, CONSTANT)]
        type ThemeEngine = super::ThemeEngineRust;

        fn seed_color(self: &ThemeEngine) -> QColor;
        fn variant_name(self: &ThemeEngine) -> QString;
        fn variant_names(self: &ThemeEngine) -> QStringList;
        fn set_dark(self: Pin<&mut ThemeEngine>, value: bool);
        fn set_variant(self: Pin<&mut ThemeEngine>, value: QString);

        #[qsignal]
        #[cxx_name = "schemeChanged"]
        fn scheme_changed(self: Pin<&mut ThemeEngine>);
        #[qsignal]
        #[cxx_name = "seedChanged"]
        fn seed_changed(self: Pin<&mut ThemeEngine>);
        #[qsignal]
        #[cxx_name = "darkChanged"]
        fn dark_changed(self: Pin<&mut ThemeEngine>);
        #[qsignal]
        #[cxx_name = "variantChanged"]
        fn variant_changed(self: Pin<&mut ThemeEngine>);

        /// The current track's cover ('' - nothing is playing); the accent is taken from it if so configured.
        #[qinvokable]
        #[cxx_name = "setCover"]
        fn set_cover(self: Pin<&mut ThemeEngine>, source: &QString);
        #[qinvokable]
        #[cxx_name = "setSeed"]
        fn set_seed(self: Pin<&mut ThemeEngine>, color: &QColor);
        /// A path or file:// URL.
        #[qinvokable]
        #[cxx_name = "setSeedFromImage"]
        fn set_seed_from_image(self: Pin<&mut ThemeEngine>, source: &QString);
    }

    impl cxx_qt::Threading for ThemeEngine {}
    impl cxx_qt::Initialize for ThemeEngine {}
}

/// Ultraviolet - the signature accent: when nothing is playing and no custom color is set. The light and dark schemes are derived from it.
pub(crate) const BRAND_SEED: &str = "#8C6DFF";
const DEFAULT_SEED: u32 = 0xFF8C6DFF;
/// The former customSeed default (Spotify green).
const LEGACY_SEED: &str = "#1DB954";

/// customSeed with the brand in mind: while the store returns the former green as the default, it means "not set".
pub(crate) fn custom_seed(store: &SettingsStore) -> String {
    let raw = store.get_str(SettingKey::CustomSeed);
    let legacy_default = matches!(SettingKey::CustomSeed.default_value(), SettingValue::Str(d) if d.eq_ignore_ascii_case(LEGACY_SEED));
    if raw.trim().is_empty() || (legacy_default && raw.trim().eq_ignore_ascii_case(LEGACY_SEED)) {
        BRAND_SEED.to_owned()
    } else {
        raw
    }
}

pub struct ThemeEngineRust {
    colors: QVariantMap,
    seed: u32,
    dark: bool,
    variant: Variant,
    contrast: f64,
    request: u64,
    pending_image: Option<PathBuf>,
    /// the current track's cover (path), even if the accent is not from it
    cover: Option<PathBuf>,
}

// colors is filled in by initialize (synchronously, by settings)
impl Default for ThemeEngineRust {
    fn default() -> Self {
        Self {
            colors: QVariantMap::default(),
            seed: DEFAULT_SEED,
            dark: true,
            variant: Variant::Content,
            contrast: 0.0,
            request: 0,
            pending_image: None,
            cover: None,
        }
    }
}

fn argb_color(argb: u32) -> QColor {
    let [a, r, g, b] = argb.to_be_bytes();
    QColor::from_rgba(r as i32, g as i32, b as i32, a as i32)
}

fn color_argb(color: &QColor) -> u32 {
    let channel = |v: i32| v.clamp(0, 255) as u32;
    (channel(color.alpha()) << 24) | (channel(color.red()) << 16) | (channel(color.green()) << 8) | channel(color.blue())
}

fn to_qcolors(scheme: &Scheme) -> QVariantMap {
    let mut map = QVariantMap::default();
    for (name, argb) in &scheme.colors {
        map.insert(QString::from(*name), QVariant::from(&argb_color(*argb)));
    }
    map
}

/// (dark, variant, seed) from the themeMode / schemeVariant / customSeed settings and the system theme.
fn settings_state() -> (bool, Variant, u32) {
    let store = &ctx().services.settings;
    let mode = store.get_str(SettingKey::ThemeMode);
    let dark = if mode == "system" { *ctx().system_dark.borrow() } else { mode != "light" };
    let variant = Variant::from_name(&store.get_str(SettingKey::SchemeVariant)).unwrap_or(Variant::Content);
    let seed = mf_theme::parse_color(&custom_seed(store)).unwrap_or(DEFAULT_SEED);
    (dark, variant, seed)
}

fn non_empty_path(path: String) -> Option<PathBuf> {
    (!path.is_empty()).then(|| PathBuf::from(path))
}

impl qobject::ThemeEngine {
    fn seed_color(&self) -> QColor {
        argb_color(self.seed)
    }

    fn variant_name(&self) -> QString {
        QString::from(self.variant.name())
    }

    fn variant_names(&self) -> QStringList {
        Variant::ALL.iter().map(|v| QString::from(v.name())).collect()
    }

    fn set_dark(mut self: Pin<&mut Self>, value: bool) {
        if value != self.dark {
            self.as_mut().rust_mut().dark = value;
            self.as_mut().dark_changed();
            self.rebuild(None);
        }
    }

    fn set_variant(mut self: Pin<&mut Self>, value: QString) {
        let Some(variant) = Variant::from_name(&value.to_string()) else { return };
        if variant != self.variant {
            self.as_mut().rust_mut().variant = variant;
            self.as_mut().variant_changed();
            self.rebuild(None);
        }
    }

    fn set_cover(mut self: Pin<&mut Self>, source: &QString) {
        let cover = non_empty_path(local_path(source));
        self.as_mut().rust_mut().cover = cover.clone();
        if ctx().services.settings.get_bool(SettingKey::AccentFromCover) {
            match cover {
                Some(path) => self.rebuild(Some(path)),
                None => self.apply_settings(),
            }
        }
    }

    fn set_seed(mut self: Pin<&mut Self>, color: &QColor) {
        let mut this = self.as_mut().rust_mut();
        this.pending_image = None;
        this.seed = color_argb(color);
        self.as_mut().seed_changed();
        self.rebuild(None);
    }

    fn set_seed_from_image(self: Pin<&mut Self>, source: &QString) {
        if let Some(path) = non_empty_path(local_path(source)) {
            self.rebuild(Some(path));
        }
    }

    /// themeMode/schemeVariant/accentFromCover/customSeed or the system theme changed.
    fn apply_settings(mut self: Pin<&mut Self>) {
        let (dark, variant, seed) = settings_state();
        if dark != self.dark {
            self.as_mut().rust_mut().dark = dark;
            self.as_mut().dark_changed();
        }
        if variant != self.variant {
            self.as_mut().rust_mut().variant = variant;
            self.as_mut().variant_changed();
        }
        let cover = self.cover.clone();
        match cover {
            Some(path) if ctx().services.settings.get_bool(SettingKey::AccentFromCover) => self.rebuild(Some(path)),
            _ => {
                let mut this = self.as_mut().rust_mut();
                this.pending_image = None;
                this.seed = seed;
                self.as_mut().seed_changed();
                self.rebuild(None);
            }
        }
    }

    /// Recalculation in the thread pool; `image` None - take the not yet parsed cover (if there is one).
    fn rebuild(mut self: Pin<&mut Self>, image: Option<PathBuf>) {
        let thread = self.qt_thread();
        let mut this = self.as_mut().rust_mut();
        let image = image.or_else(|| this.pending_image.clone());
        this.pending_image = image.clone();
        this.request += 1;
        let (request, seed, dark, variant, contrast) = (this.request, this.seed, this.dark, this.variant, this.contrast);
        ctx().rt.spawn_blocking(move || {
            let seed = match &image {
                Some(path) => match mf_theme::seed_from_image(path) {
                    Ok(seed) => seed,
                    Err(error) => {
                        tracing::debug!("theme: {error}");
                        return;
                    }
                },
                None => seed,
            };
            let scheme = mf_theme::build_scheme(seed, dark, variant, contrast);
            let _ = thread.queue(move |theme| theme.scheme_ready(request, scheme));
        });
    }

    fn scheme_ready(mut self: Pin<&mut Self>, request: u64, scheme: Scheme) {
        if request != self.request {
            return;
        }
        self.as_mut().rust_mut().pending_image = None;
        if scheme.seed != self.seed {
            self.as_mut().rust_mut().seed = scheme.seed;
            self.as_mut().seed_changed();
        }
        self.as_mut().rust_mut().colors = to_qcolors(&scheme);
        self.scheme_changed();
    }

    fn on_settings(self: Pin<&mut Self>, delivery: Delivery<SettingKey>) {
        let relevant = match delivery {
            Delivery::Event(key) => matches!(
                key,
                SettingKey::ThemeMode | SettingKey::SchemeVariant | SettingKey::AccentFromCover | SettingKey::CustomSeed
            ),
            Delivery::Lagged => true,
        };
        if relevant {
            self.apply_settings();
        }
    }

    fn on_player(self: Pin<&mut Self>, delivery: Delivery<PlayerEvent>) {
        if matches!(delivery, Delivery::Event(PlayerEvent::CoverFileChanged) | Delivery::Lagged) {
            let file = ctx().services.player.snapshot().cover_file;
            self.set_cover(&QString::from(file.as_str()));
        }
    }
}

impl cxx_qt::Initialize for qobject::ThemeEngine {
    fn initialize(mut self: Pin<&mut Self>) {
        // the starting scheme synchronously: the window does not "repaint" at startup
        let (dark, variant, seed) = settings_state();
        {
            let mut this = self.as_mut().rust_mut();
            this.dark = dark;
            this.variant = variant;
            this.seed = seed;
            this.colors = to_qcolors(&mf_theme::build_scheme(seed, dark, variant, this.contrast));
        }
        self.as_mut().dark_changed();
        self.as_mut().variant_changed();
        self.as_mut().seed_changed();
        self.as_mut().scheme_changed();

        let context = ctx();
        forward_events(self.qt_thread(), context.services.settings.subscribe(), Self::on_settings);
        forward_events(self.qt_thread(), context.services.player.subscribe().events, Self::on_player);
        let thread = self.qt_thread();
        let mut system_dark = context.system_dark.clone();
        context.rt.spawn(async move {
            while system_dark.changed().await.is_ok() {
                if thread.queue(|theme| theme.apply_settings()).is_err() {
                    break;
                }
            }
        });

        // the restored session's cover may have downloaded before the subscription
        let cover_file = context.services.player.snapshot().cover_file;
        if !cover_file.is_empty() {
            self.as_mut().set_cover(&QString::from(cover_file.as_str()));
        }
        // one-off development flags: not written to settings
        if context.flags.light {
            self.as_mut().set_dark(false);
        }
        if let Some(path) = &context.flags.seed_image {
            let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
            self.as_mut().set_seed_from_image(&QString::from(path.to_string_lossy().as_ref()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_seed_until_custom_seed_is_chosen() {
        let store = SettingsStore::in_memory();
        assert!(custom_seed(&store).eq_ignore_ascii_case(BRAND_SEED));
        assert_eq!(mf_theme::parse_color(&custom_seed(&store)), Some(DEFAULT_SEED));
        store.set(SettingKey::CustomSeed, SettingValue::Str("#FF6D00".into()));
        assert_eq!(custom_seed(&store), "#FF6D00");
    }
}
