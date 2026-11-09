// Auth — зеркало AuthService: свойства из снимка (watch), сигналы-факты — из событий.
// Основной вход — сессия librespot (браузер, :8898, Premium): с ним и медиатека, и звук здесь.
// Вход Web API (:8989) необязателен: «ускорить медиатеку» и вход без Premium.
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use yt_services::{AuthEvent, AuthSnapshot};

use super::{Delivery, forward_events, forward_watch, to_qmap};
use crate::context::ctx;
use crate::qt::QVariantMap;

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
        #[qproperty(QString, state, READ, NOTIFY = state_changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = error_changed)]
        /// стабильный код errorText для подсказок (текст переведён): "" | "free" | "cancelled" | "portBusy" |
        /// "noResponse" | "accountMismatch" | "otherAccount" | "audioKey"
        #[qproperty(QString, error_code, cxx_name = "errorCode", READ, NOTIFY = error_changed)]
        #[qproperty(QString, auth_url, cxx_name = "authUrl", READ, NOTIFY = auth_url_changed)]
        #[qproperty(i32, auth_port, cxx_name = "authPort", READ, NOTIFY = auth_url_changed)]
        #[qproperty(QString, playback_state, cxx_name = "playbackState", READ, NOTIFY = playback_changed)]
        #[qproperty(QString, playback_error, cxx_name = "playbackError", READ, NOTIFY = playback_changed)]
        /// код playbackError, значения как у errorCode
        #[qproperty(QString, playback_error_code, cxx_name = "playbackErrorCode", READ, NOTIFY = playback_changed)]
        #[qproperty(QString, token_storage, cxx_name = "tokenStorage", READ, NOTIFY = storage_changed)]
        #[qproperty(QMap_QString_QVariant, account, READ, NOTIFY = account_changed)]
        #[qproperty(bool, premium, READ, NOTIFY = account_changed)]
        #[qproperty(QString, personal_client_id, cxx_name = "personalClientId", READ, NOTIFY = personal_changed)]
        #[qproperty(QString, personal_state, cxx_name = "personalState", READ, NOTIFY = personal_changed)]
        /// redirect URI to enter in the personal app's settings (the Web API apps share it)
        #[qproperty(QString, personal_redirect_uri, cxx_name = "personalRedirectUri", READ, CONSTANT)]
        /// токен общего Web API: "none" | "authorizing" | "ready" | "error"
        #[qproperty(QString, web_state, cxx_name = "webState", READ, NOTIFY = web_changed)]
        type Auth = super::AuthRust;

        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut Auth>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut Auth>);
        #[qsignal]
        #[cxx_name = "authUrlChanged"]
        fn auth_url_changed(self: Pin<&mut Auth>);
        #[qsignal]
        #[cxx_name = "playbackChanged"]
        fn playback_changed(self: Pin<&mut Auth>);
        #[qsignal]
        #[cxx_name = "storageChanged"]
        fn storage_changed(self: Pin<&mut Auth>);
        #[qsignal]
        #[cxx_name = "accountChanged"]
        fn account_changed(self: Pin<&mut Auth>);
        #[qsignal]
        #[cxx_name = "personalChanged"]
        fn personal_changed(self: Pin<&mut Auth>);
        #[qsignal]
        #[cxx_name = "webChanged"]
        fn web_changed(self: Pin<&mut Auth>);
        #[qsignal]
        #[cxx_name = "signedIn"]
        fn signed_in(self: Pin<&mut Auth>);
        #[qsignal]
        #[cxx_name = "signedOut"]
        fn signed_out(self: Pin<&mut Auth>);
        /// воспроизведение на этом ПК готово
        #[qsignal]
        #[cxx_name = "playbackReady"]
        fn playback_ready(self: Pin<&mut Auth>);
        /// вошли, но аккаунт Free (показать PremiumNotice один раз)
        #[qsignal]
        #[cxx_name = "premiumRequired"]
        fn premium_required(self: Pin<&mut Auth>);

        /// «Войти через Spotify» (Premium): браузер + 127.0.0.1:8898, вход через сессию librespot.
        #[qinvokable]
        #[cxx_name = "startLogin"]
        fn start_login(self: &Auth);
        /// «Войти без Premium» / «Ускорить медиатеку»: вход Web API, браузер + 127.0.0.1:8989.
        #[qinvokable]
        #[cxx_name = "startWebLogin"]
        fn start_web_login(self: &Auth);
        /// «Открыть снова» (authUrl в браузере).
        #[qinvokable]
        #[cxx_name = "reopenBrowser"]
        fn reopen_browser(self: &Auth);
        /// «Ссылка» — authUrl в буфер обмена.
        #[qinvokable]
        #[cxx_name = "copyAuthUrl"]
        fn copy_auth_url(self: &Auth);
        #[qinvokable]
        #[cxx_name = "cancelLogin"]
        fn cancel_login(self: &Auth);
        #[qinvokable]
        #[cxx_name = "signOut"]
        fn sign_out(self: &Auth);
        #[qinvokable]
        fn retry(self: &Auth);
        /// «Проверить снова» (Premium).
        #[qinvokable]
        #[cxx_name = "refreshProfile"]
        fn refresh_profile(self: &Auth);
        /// «Разрешить воспроизведение» (браузер + 127.0.0.1:8898).
        #[qinvokable]
        #[cxx_name = "authorizePlayback"]
        fn authorize_playback(self: &Auth);
        /// «Отключить» воспроизведение на этом ПК.
        #[qinvokable]
        #[cxx_name = "disablePlayback"]
        fn disable_playback(self: &Auth);
        /// Свой Client ID ("" — удалить).
        #[qinvokable]
        #[cxx_name = "setPersonalClientId"]
        fn set_personal_client_id(self: &Auth, client_id: &QString);
        #[qinvokable]
        #[cxx_name = "copyToClipboard"]
        fn copy_to_clipboard(self: &Auth, text: &QString);
        /// Save pasted ytmusicapi headers-auth JSON and validate it (in-app login, no files).
        /// Returns "" on success or an error message to show.
        #[qinvokable]
        #[cxx_name = "saveHeaders"]
        fn save_headers(self: &Auth, json: &QString) -> QString;
    }

    impl cxx_qt::Threading for Auth {}
    impl cxx_qt::Initialize for Auth {}
}

pub struct AuthRust {
    snapshot: AuthSnapshot,
    state: QString,
    error_text: QString,
    error_code: QString,
    auth_url: QString,
    auth_port: i32,
    playback_state: QString,
    playback_error: QString,
    playback_error_code: QString,
    token_storage: QString,
    account: QVariantMap,
    premium: bool,
    personal_client_id: QString,
    personal_state: QString,
    personal_redirect_uri: QString,
    web_state: QString,
}

impl Default for AuthRust {
    fn default() -> Self {
        let snapshot = AuthSnapshot::default();
        Self {
            state: QString::from(snapshot.state.as_str()),
            error_text: QString::default(),
            error_code: QString::default(),
            auth_url: QString::default(),
            auth_port: 0,
            playback_state: QString::from(snapshot.playback.as_str()),
            playback_error: QString::default(),
            playback_error_code: QString::default(),
            token_storage: QString::default(),
            account: QVariantMap::default(),
            premium: false,
            personal_client_id: QString::default(),
            personal_state: QString::default(),
            personal_redirect_uri: QString::from(yt_services::Grant::shared_web_api().redirect_uri().as_str()),
            web_state: QString::default(),
            snapshot,
        }
    }
}

impl qobject::Auth {
    fn start_login(&self) {
        ctx().services.auth.start_login();
    }

    fn start_web_login(&self) {
        ctx().services.auth.start_web_login();
    }

    fn reopen_browser(&self) {
        ctx().services.auth.reopen_browser();
    }

    fn copy_auth_url(&self) {
        crate::qt::copy_to_clipboard(&self.auth_url.to_string());
    }

    fn cancel_login(&self) {
        ctx().services.auth.cancel_login();
    }

    fn sign_out(&self) {
        ctx().services.auth.sign_out();
    }

    fn retry(&self) {
        ctx().services.auth.retry();
    }

    fn refresh_profile(&self) {
        ctx().services.auth.refresh_profile();
    }

    fn authorize_playback(&self) {
        ctx().services.auth.authorize_playback();
    }

    fn disable_playback(&self) {
        ctx().services.auth.disable_playback();
    }

    fn set_personal_client_id(&self, client_id: &QString) {
        let id = client_id.to_string().trim().to_owned();
        ctx().services.auth.set_personal_client_id((!id.is_empty()).then_some(id));
    }

    fn copy_to_clipboard(&self, text: &QString) {
        crate::qt::copy_to_clipboard(&text.to_string());
    }

    fn save_headers(&self, json: &QString) -> QString {
        let json_str = json.to_string();
        match ctx().services.auth.save_headers(&json_str) {
            Ok(()) => QString::default(),
            Err(e) => QString::from(e),
        }
    }

    /// Снимок сервиса → свойства; сигнал — только для изменившихся групп.
    /// QML знает «signedOut» | «checking» | «signedIn» | «error»; yt-services
    /// говорит «idle» | «waiting» | «ready» | «failed» — переводим здесь.
    fn apply(mut self: Pin<&mut Self>, next: AuthSnapshot) {
        let prev = std::mem::replace(&mut self.as_mut().rust_mut().snapshot, next.clone());
        if prev == next {
            return;
        }
        if prev.state != next.state {
            let qml_state = match next.state {
                yt_services::AuthState::Idle => "signedOut",
                yt_services::AuthState::Waiting => "checking",
                yt_services::AuthState::Ready => "signedIn",
                yt_services::AuthState::Failed => "error",
            };
            self.as_mut().rust_mut().state = QString::from(qml_state);
            self.as_mut().state_changed();
        }
        if prev.error_text != next.error_text || prev.error_code != next.error_code {
            let mut this = self.as_mut().rust_mut();
            this.error_text = QString::from(next.error_text.as_str());
            this.error_code = QString::from(next.error_code.as_str());
            self.as_mut().error_changed();
        }
        if prev.auth_url != next.auth_url || prev.auth_port != next.auth_port {
            let mut this = self.as_mut().rust_mut();
            this.auth_url = QString::from(next.auth_url.as_str());
            this.auth_port = i32::from(next.auth_port);
            self.as_mut().auth_url_changed();
        }
        if prev.playback != next.playback
            || prev.playback_error != next.playback_error
            || prev.playback_error_code != next.playback_error_code
        {
            let mut this = self.as_mut().rust_mut();
            let qml_playback = match next.playback {
                yt_services::PlaybackAuthState::Authorized => "ready",
                yt_services::PlaybackAuthState::Authorizing => "connecting",
                yt_services::PlaybackAuthState::Unavailable | yt_services::PlaybackAuthState::Failed => "unavailable",
            };
            this.playback_state = QString::from(qml_playback);
            this.playback_error = QString::from(next.playback_error.as_str());
            this.playback_error_code = QString::from(next.playback_error_code.as_str());
            self.as_mut().playback_changed();
        }
        if prev.token_storage != next.token_storage {
            self.as_mut().rust_mut().token_storage = QString::from(next.token_storage.as_str());
            self.as_mut().storage_changed();
        }
        if prev.account != next.account {
            let mut this = self.as_mut().rust_mut();
            this.account = next.account.as_ref().map(to_qmap).unwrap_or_default();
            this.premium = next.account.as_ref().is_some_and(|a| a.premium);
            self.as_mut().account_changed();
        }
        if prev.personal_client_id != next.personal_client_id || prev.personal_state != next.personal_state {
            let mut this = self.as_mut().rust_mut();
            this.personal_client_id = QString::from(next.personal_client_id.as_str());
            this.personal_state = QString::from(next.personal_state.as_str());
            self.as_mut().personal_changed();
        }
        if prev.web_state != next.web_state {
            self.as_mut().rust_mut().web_state = QString::from(next.web_state.as_str());
            self.as_mut().web_changed();
        }
    }

    fn on_snapshot(self: Pin<&mut Self>, snapshot: AuthSnapshot) {
        self.apply(snapshot);
    }

    fn on_event(mut self: Pin<&mut Self>, delivery: Delivery<AuthEvent>) {
        // снимок обновлён до рассылки события: сначала свойства, потом сигнал
        let snapshot = ctx().services.auth.snapshot();
        self.as_mut().apply(snapshot);
        match delivery {
            Delivery::Event(AuthEvent::SignedIn(_)) => self.signed_in(),
            Delivery::Event(AuthEvent::SignedOut) => self.signed_out(),
            Delivery::Event(AuthEvent::PlaybackReady(_)) => self.playback_ready(),
            Delivery::Event(AuthEvent::PremiumRequired) => self.premium_required(),
            Delivery::Lagged => {}
        }
    }
}

impl cxx_qt::Initialize for qobject::Auth {
    fn initialize(mut self: Pin<&mut Self>) {
        let auth = &ctx().services.auth;
        let mut watch = auth.watch();
        let snapshot = watch.borrow_and_update().clone();
        self.as_mut().apply(snapshot);
        forward_watch(self.qt_thread(), watch, Self::on_snapshot);
        forward_events(self.qt_thread(), auth.subscribe(), Self::on_event);
    }
}
