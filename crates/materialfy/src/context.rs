//! AppContext — интерфейс между оболочкой («shell») и мостами («bridges»).
//! Оболочка создаёт его один раз (до регистрации типов QML), мосты только читают через [`ctx()`].
use std::path::PathBuf;
use std::sync::OnceLock;

use tokio::runtime::Handle;
use tokio::sync::watch;

/// Флаги запуска, нужные мостам.
#[derive(Clone, Debug, Default)]
pub struct StartupFlags {
    /// --screenshot: режим снимка (без трея, MPRIS, сессии)
    pub screenshot: bool,
    /// --light: ThemeEngine.dark = false один раз после привязки к настройкам (в настройки не пишется)
    pub light: bool,
    /// --seed-image: ThemeEngine.setSeedFromImage(путь) после привязки к настройкам
    pub seed_image: Option<PathBuf>,
}

pub struct AppContext {
    /// tokio-рантайм (многопоточный); мосты делают rt.spawn(...) и возвращаются в GUI через CxxQtThread::queue
    pub rt: Handle,
    pub services: yt_services::Services,
    pub flags: StartupFlags,
    /// системная тёмная тема (QStyleHints::colorScheme), обновляется оболочкой по colorSchemeChanged
    pub system_dark: watch::Receiver<bool>,
}

static CONTEXT: OnceLock<AppContext> = OnceLock::new();

/// Вызывает оболочка ровно один раз, до qt::ffi::register_qml_types().
pub fn init(context: AppContext) {
    if CONTEXT.set(context).is_err() {
        panic!("AppContext already created");
    }
}

/// Контекст приложения. Паника, если оболочка ещё не вызвала init (ошибка порядка запуска).
pub fn ctx() -> &'static AppContext {
    CONTEXT.get().expect("AppContext not created: call context::init() before registering QML types")
}

pub fn try_ctx() -> Option<&'static AppContext> {
    CONTEXT.get()
}
