//! Single instance (app.py: raise_running_instance).
//! Linux: if mpris::BUS_NAME is already on the session bus — call org.mpris.MediaPlayer2.Raise on it
//! and return true (the caller exits with code 0). The running instance receives the request as MprisCommand::Raise.
//!
//! Windows and macOS (an addition to the Python version, where it was always false there): the running instance holds
//! [`InstanceListener`] — a named pipe `\\.\pipe\materialfy-Materialfy-<user>` (created as the pipe's first instance
//! pipe, so it doubles as a mutex) or a unix socket `$TMPDIR/materialfy-Materialfy.sock`. The new process connects,
//! writes "raise\n" and exits; with no listener (the shell did not start it) — false, as before.
//! Protocol: the client sends the string "raise\n", the server replies "ok\n" or "hung\n": the window is not responding (the GUI thread has not
//! checked in with [`gui_alive`] for 20 s: a stall on a heavy page on a weak PC is not yet a hang). A hung instance is terminated by the new process (Windows) and that process starts itself —
//! otherwise after a hang every launch quietly exited, "showing" a dead window.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::PlatformError;

#[cfg(any(target_os = "windows", target_os = "macos"))]
const REQUEST: &[u8] = b"raise\n";
#[cfg(any(target_os = "windows", target_os = "macos"))]
const REPLY: &[u8] = b"ok\n";
#[cfg(any(target_os = "windows", target_os = "macos"))]
const HUNG: &[u8] = b"hung\n";

/// The GUI thread's last check-in and the listener's start, ms since the epoch (0 — none yet).
static GUI_BEAT_MS: AtomicU64 = AtomicU64::new(0);
static LISTEN_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The GUI thread is alive — call from its timer once a second.
pub fn gui_alive() {
    GUI_BEAT_MS.store(now_ms(), Ordering::Relaxed);
}

/// The window is responding: the check-in is fresh; before the first check-in (the window is still loading) — the first 60 s of running.
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
fn gui_responsive() -> bool {
    let now = now_ms();
    match GUI_BEAT_MS.load(Ordering::Relaxed) {
        0 => now.saturating_sub(LISTEN_MS.load(Ordering::Relaxed)) < 60_000,
        beat => now.saturating_sub(beat) < 20_000,
    }
}
const TIMEOUT: Duration = Duration::from_secs(3);

/// true — a running instance was found and asked to show its window.
pub async fn raise_running_instance() -> bool {
    match tokio::time::timeout(TIMEOUT, imp::raise()).await {
        Ok(found) => found,
        Err(_) => {
            tracing::debug!("the running instance did not answer in time");
            false
        }
    }
}

/// How long the new process waits for the already-running (possibly still starting) instance to answer.
#[cfg(target_os = "windows")]
const PRIMARY_WAIT: Duration = Duration::from_secs(20);

/// Become the single instance. true — this process is first, carry on; false — another one is already
/// running (its window is shown) — exit. Windows: the named mutex is taken atomically from the very start of launch,
/// so two simultaneous launches (a double click, autostart and a shortcut) do not both start, even while the first
/// has not yet opened the request channel. A hung instance is terminated by raise — then this process takes its place.
pub async fn become_primary() -> bool {
    #[cfg(target_os = "windows")]
    {
        let deadline = std::time::Instant::now() + PRIMARY_WAIT;
        loop {
            if imp::claim() {
                // the mutex is free, but the pipe may be held by a version from before the mutex — show its window
                return !raise_running_instance().await;
            }
            if raise_running_instance().await {
                return false;
            }
            if std::time::Instant::now() >= deadline {
                tracing::warn!("another Materialfy is starting but doesn't answer; not starting a second one");
                return false;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        !raise_running_instance().await
    }
}

/// A listener for "show the window" requests from new processes (Windows/macOS).
/// Not needed on Linux: `start` returns Err(Unsupported), the request arrives via MPRIS.
pub struct InstanceListener {
    requests: mpsc::UnboundedReceiver<()>,
    task: tokio::task::JoinHandle<()>,
}

impl InstanceListener {
    /// Take the pipe/socket. An error means another instance is already listening (or the OS is unsupported).
    /// Call inside a tokio runtime.
    pub async fn start() -> Result<InstanceListener, PlatformError> {
        let (tx, requests) = mpsc::unbounded_channel();
        LISTEN_MS.store(now_ms(), Ordering::Relaxed);
        let task = imp::listen(tx).await?;
        Ok(InstanceListener { requests, task })
    }

    /// Wait for the next show-the-window request; None — the listener has stopped.
    pub async fn recv(&mut self) -> Option<()> {
        self.requests.recv().await
    }

    /// Hand over the request channel (the listener keeps working while the runtime lives).
    pub fn into_receiver(self) -> mpsc::UnboundedReceiver<()> {
        self.requests
    }

    pub fn close(self) {
        self.task.abort();
    }
}

/// Handle a single connection: read the request, reply.
#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn serve_client<S>(mut stream: S, tx: mpsc::UnboundedSender<()>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = [0u8; 16];
    let mut len = 0;
    let read = tokio::time::timeout(TIMEOUT, async {
        while len < buf.len() {
            match stream.read(&mut buf[len..]).await {
                Ok(0) | Err(_) => break,
                Ok(n) => len += n,
            }
            if buf[..len].contains(&b'\n') {
                break;
            }
        }
    });
    let _ = read.await;
    if buf[..len] == *REQUEST {
        let reply = if gui_responsive() {
            let _ = tx.send(());
            REPLY
        } else {
            tracing::warn!("asked to show the window, but the GUI thread does not respond");
            HUNG
        };
        let _ = stream.write_all(reply).await;
        let _ = stream.flush().await;
    }
}

/// A reply from the running instance.
#[cfg(any(target_os = "windows", target_os = "macos"))]
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Shown,
    Hung,
}

/// Client: send the request and wait for the reply. None — the request did not go out.
#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn send_request<S>(mut stream: S) -> Option<Answer>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    if stream.write_all(REQUEST).await.is_err() || stream.flush().await.is_err() {
        return None;
    }
    let mut reply = [0u8; 8];
    match stream.read(&mut reply).await {
        Ok(n) if reply[..n] == *HUNG => Some(Answer::Hung),
        // the request went out even if the reply did not arrive — the second instance is not needed anyway
        _ => Some(Answer::Shown),
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use tokio::sync::mpsc;
    use tokio::task::JoinHandle;

    use crate::PlatformError;
    use crate::mpris::{BUS_NAME, OBJECT_PATH};

    pub async fn raise() -> bool {
        match try_raise().await {
            Ok(found) => found,
            Err(err) => {
                tracing::debug!("D-Bus unavailable: {err}");
                false
            }
        }
    }

    async fn try_raise() -> zbus::Result<bool> {
        let conn = zbus::Connection::session().await?;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        let name = zbus::names::BusName::try_from(BUS_NAME)?;
        if !dbus.name_has_owner(name).await? {
            return Ok(false);
        }
        let reply = conn
            .call_method(Some(BUS_NAME), OBJECT_PATH, Some("org.mpris.MediaPlayer2"), "Raise", &())
            .await;
        Ok(reply.is_ok())
    }

    pub async fn listen(_tx: mpsc::UnboundedSender<()>) -> Result<JoinHandle<()>, PlatformError> {
        Err(PlatformError::Unsupported)
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::time::Duration;

    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
    use tokio::sync::mpsc;
    use tokio::task::JoinHandle;

    use crate::PlatformError;
    use crate::dirs::{APPLICATION, ORGANIZATION};

    const ERROR_PIPE_BUSY: i32 = 231;

    pub fn pipe_name() -> String {
        let user: String = std::env::var("USERNAME")
            .unwrap_or_default()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        format!(r"\\.\pipe\{ORGANIZATION}-{APPLICATION}-{user}")
    }

    pub async fn raise() -> bool {
        let name = pipe_name();
        let mut attempts = 0;
        let client = loop {
            match ClientOptions::new().open(&name) {
                Ok(client) => break client,
                Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempts < 20 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(_) => return false,
            }
        };
        let pid = allow_foreground(&client);
        match super::send_request(client).await {
            Some(super::Answer::Shown) => true,
            Some(super::Answer::Hung) => {
                // the window hung: terminate that process and start in its place
                if let Some(pid) = pid {
                    terminate(pid).await;
                }
                false
            }
            None => false,
        }
    }

    /// Allow the running instance to bring its window to the front (otherwise Windows only flashes the taskbar button).
    /// Returns its PID.
    fn allow_foreground(client: &impl AsRawHandle) -> Option<u32> {
        use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
        let mut pid = 0u32;
        // SAFETY: the open pipe handle outlives the call, pid is a valid pointer
        unsafe {
            if GetNamedPipeServerProcessId(client.as_raw_handle(), &mut pid) != 0 && pid != 0 {
                AllowSetForegroundWindow(pid);
                return Some(pid);
            }
        }
        None
    }

    /// Terminate the hung instance and wait for it to release the pipe.
    async fn terminate(pid: u32) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
        tracing::warn!("the running instance (pid {pid}) does not respond: ending it");
        // SAFETY: the process handle is opened here and closed after the call
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !process.is_null() {
                TerminateProcess(process, 1);
                CloseHandle(process);
            }
        }
        for _ in 0..40 {
            if ClientOptions::new().open(pipe_name()).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Take the instance mutex for the process's whole lifetime; false — another process holds it.
    pub fn claim() -> bool {
        use std::sync::OnceLock;
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        static HELD: OnceLock<usize> = OnceLock::new();
        if HELD.get().is_some() {
            return true;
        }
        let name: Vec<u16> = format!(r"Local\{ORGANIZATION}-{APPLICATION}-instance").encode_utf16().chain([0]).collect();
        // SAFETY: the name is a null-terminated string; the handle is either closed here or lives until the process ends
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if handle.is_null() {
                // cannot create the mutex (permissions, a strange environment) — do not block the launch, the pipe remains
                return true;
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                return false;
            }
            let _ = HELD.set(handle as usize);
        }
        true
    }

    fn create(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
        ServerOptions::new().first_pipe_instance(first).create(name)
    }

    pub async fn listen(tx: mpsc::UnboundedSender<()>) -> Result<JoinHandle<()>, PlatformError> {
        let name = pipe_name();
        let mut server = create(&name, true)?;
        Ok(tokio::spawn(async move {
            loop {
                if let Err(err) = server.connect().await {
                    tracing::debug!("single-instance channel: {err}");
                }
                let next = match create(&name, false) {
                    Ok(next) => next,
                    Err(err) => {
                        tracing::warn!("single-instance channel closed: {err}");
                        return;
                    }
                };
                let client = std::mem::replace(&mut server, next);
                tokio::spawn(super::serve_client(client, tx.clone()));
            }
        }))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::PathBuf;

    use tokio::net::{UnixListener, UnixStream};
    use tokio::sync::mpsc;
    use tokio::task::JoinHandle;

    use crate::PlatformError;
    use crate::dirs::{APPLICATION, ORGANIZATION};

    pub fn socket_path() -> PathBuf {
        // $TMPDIR on macOS is per user
        std::env::temp_dir().join(format!("{ORGANIZATION}-{APPLICATION}.sock"))
    }

    pub async fn raise() -> bool {
        match UnixStream::connect(socket_path()).await {
            // on macOS we do not terminate the hung instance — just start
            Ok(stream) => super::send_request(stream).await == Some(super::Answer::Shown),
            Err(_) => false,
        }
    }

    pub async fn listen(tx: mpsc::UnboundedSender<()>) -> Result<JoinHandle<()>, PlatformError> {
        let path = socket_path();
        if UnixStream::connect(&path).await.is_ok() {
            return Err(std::io::Error::from(std::io::ErrorKind::AddrInUse).into());
        }
        // a socket left by a crashed process
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        Ok(tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        tokio::spawn(super::serve_client(stream, tx.clone()));
                    }
                    Err(err) => {
                        tracing::warn!("single-instance socket closed: {err}");
                        return;
                    }
                }
            }
        }))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod imp {
    use tokio::sync::mpsc;
    use tokio::task::JoinHandle;

    use crate::PlatformError;

    pub async fn raise() -> bool {
        false
    }

    pub async fn listen(_tx: mpsc::UnboundedSender<()>) -> Result<JoinHandle<()>, PlatformError> {
        Err(PlatformError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    /// The window check-in is per process: tests that touch it run in sequence
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    static GUI_STATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[tokio::test]
    async fn protocol_roundtrip() {
        let _serial = GUI_STATE.lock().await;
        super::gui_alive();
        let (client, server) = tokio::io::duplex(64);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let served = tokio::spawn(super::serve_client(server, tx));
        assert_eq!(super::send_request(client).await, Some(super::Answer::Shown));
        served.await.unwrap();
        assert_eq!(rx.recv().await, Some(()));
    }

    /// The window has not checked in for a long time — the reply is "hung", the show-the-window request is not sent.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[tokio::test]
    async fn a_hung_window_answers_hung() {
        use std::sync::atomic::Ordering;
        let _serial = GUI_STATE.lock().await;
        super::GUI_BEAT_MS.store(super::now_ms() - 30_000, Ordering::Relaxed);
        let (client, server) = tokio::io::duplex(64);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let served = tokio::spawn(super::serve_client(server, tx));
        assert_eq!(super::send_request(client).await, Some(super::Answer::Hung));
        served.await.unwrap();
        assert!(rx.try_recv().is_err());
        super::gui_alive();
        assert!(super::gui_responsive());
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[tokio::test]
    async fn garbage_is_ignored() {
        use tokio::io::AsyncWriteExt;
        let (mut client, server) = tokio::io::duplex(64);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let served = tokio::spawn(super::serve_client(server, tx));
        client.write_all(b"hello\n").await.unwrap();
        served.await.unwrap();
        assert!(rx.try_recv().is_err());
    }

    /// A real pipe/socket: it takes the user's name, so it is not the default.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[tokio::test]
    #[ignore = "takes the single-instance name"]
    async fn listener_receives_raise() {
        let mut listener = super::InstanceListener::start().await.unwrap();
        assert!(super::InstanceListener::start().await.is_err());
        assert!(super::raise_running_instance().await);
        assert_eq!(listener.recv().await, Some(()));
        listener.close();
    }
}
