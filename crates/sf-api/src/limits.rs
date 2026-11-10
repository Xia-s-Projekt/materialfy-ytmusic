//! Web API request budget. The shared Client ID is the same for all users, and Spotify quickly answers it with 429,
//! or even QUOTA_EXCEEDED for a day. Here:
//! - priorities: the page a person opened > background refresh > player polling;
//! - a 30 s window per source: background and polling only get a part, pages get all of it;
//! - coalescing of identical GETs in flight and a short cache of their answers (any write clears it);
//! - source cooldown after 429/QUOTA_EXCEEDED: background and polling do not hit the network then, a page rarely
//!   tries, and the UI sees the "Web API limited" state.
//!
//! The caller sets the priority: `limits::background(fut)` / `limits::polling(fut)` (task-local, does not carry over to spawned
//! tasks). Without it, player polling GETs are Polling, everything else is Interactive.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use tokio::sync::watch;

use crate::ApiSource;
use crate::client::ApiError;

/// The window in which requests are counted (Spotify counts the limit over a sliding 30 s).
pub const WINDOW: Duration = Duration::from_secs(30);
/// Soft budget of the shared Client ID per window.
const SHARED_BUDGET: usize = 40;
/// A personal application serves only this user, so it may have more.
const PERSONAL_BUDGET: usize = 120;
/// Background does not wait for room in the window longer than this — it returns RateLimited.
const BACKGROUND_WAIT: Duration = Duration::from_secs(30);
/// A short 429 is waited out by the page itself (the request waiting budget in client.rs is 8 s).
const SHORT_HOLD: Duration = Duration::from_secs(8);
/// How often a page tries the source under a long cooldown.
const PROBE_RATE_LIMITED: Duration = Duration::from_secs(15);
const PROBE_QUOTA: Duration = Duration::from_secs(60);
/// Quota without a clear Retry-After: at least 10 minutes, at most a day.
const QUOTA_MIN: Duration = Duration::from_secs(10 * 60);
const QUOTA_MAX: Duration = Duration::from_secs(24 * 3600);
const RATE_LIMIT_MAX: Duration = Duration::from_secs(3600);
/// Polling backs off after a 429: 60 s, 120 s, 240 s… up to 10 minutes.
const POLL_BACKOFF: Duration = Duration::from_secs(60);
const POLL_BACKOFF_MAX: Duration = Duration::from_secs(10 * 60);
/// Cooldowns shorter than this are not shown in the UI: a one-second 429 is no reason to flash a warning.
const SHOW_AFTER: Duration = Duration::from_secs(5);
/// How long a GET answer lives in the cache.
const CACHE_TTL: Duration = Duration::from_secs(20);
/// The catalog (albums, artists, tracks, shows) changes rarely: its answers live longer.
const CATALOG_TTL: Duration = Duration::from_secs(10 * 60);
/// Own lists (/me/tracks, /me/albums…): writes through the Web API or a session clear the cache anyway.
const USER_TTL: Duration = Duration::from_secs(60);
/// After a write the cache is not filled: snapshot/cover rechecks must see a fresh answer.
const WRITE_QUIET: Duration = Duration::from_secs(60);
const CACHE_MAX: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// State polling (GET /me/player, the queue): yields first and backs off the most after a 429.
    Polling,
    /// Background refresh: library, new releases, shelves — waits for room in the window.
    Background,
    /// The page a person opened, and any writes.
    Interactive,
}

tokio::task_local! {
    static PRIORITY: Priority;
}

/// Run `future` with a Web API request priority.
pub async fn with_priority<F: Future>(priority: Priority, future: F) -> F::Output {
    PRIORITY.scope(priority, future).await
}

/// Background work: yields to pages and stays off the network while the source cools down.
pub async fn background<F: Future>(future: F) -> F::Output {
    with_priority(Priority::Background, future).await
}

/// Periodic polling: the smallest share of the window and a long backoff after a 429.
pub async fn polling<F: Future>(future: F) -> F::Output {
    with_priority(Priority::Polling, future).await
}

/// Priority of the current request.
pub(crate) fn priority_of(is_get: bool, path: &str) -> Priority {
    PRIORITY.try_with(|priority| *priority).unwrap_or(if is_get && is_polled(path) {
        Priority::Polling
    } else {
        Priority::Interactive
    })
}

fn is_polled(path: &str) -> bool {
    matches!(path, "/me/player" | "/me/player/queue" | "/me/player/currently-playing")
}

fn budget(source: ApiSource) -> usize {
    match source {
        ApiSource::Shared => SHARED_BUDGET,
        ApiSource::Personal => PERSONAL_BUDGET,
    }
}

/// The share of the window a priority may take.
fn share(priority: Priority, budget: usize) -> usize {
    match priority {
        Priority::Interactive => usize::MAX,
        // background and polling get less than half the window: a library walk at startup does not eat the pages' room
        Priority::Background => budget / 3,
        Priority::Polling => (budget / 8).max(1),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    /// 429 with Retry-After.
    RateLimited,
    /// 429 with reason QUOTA_EXCEEDED: development mode quota, usually up to a day.
    Quota,
}

impl LimitKind {
    fn error(self) -> ApiError {
        match self {
            Self::RateLimited => ApiError::RateLimited,
            Self::Quota => ApiError::QuotaExhausted,
        }
    }

    fn probe_every(self) -> Duration {
        match self {
            Self::RateLimited => PROBE_RATE_LIMITED,
            Self::Quota => PROBE_QUOTA,
        }
    }
}

/// Source cooldown as the UI sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit {
    pub kind: LimitKind,
    pub until: Instant,
}

/// Web API state for the UI: None — the source is working.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WebApiLimits {
    pub shared: Option<Limit>,
    pub personal: Option<Limit>,
}

impl WebApiLimits {
    pub fn is_limited(&self) -> bool {
        self.shared.is_some() || self.personal.is_some()
    }

    /// Explanation under the "Web API limited" heading; empty — there are no limits.
    pub fn user_text(&self) -> String {
        let line = |limit: &Limit, shared: bool| {
            let until = clock_after(limit.until.saturating_duration_since(Instant::now()));
            match (limit.kind, shared) {
                (LimitKind::Quota, true) => mf_i18n::tf("The shared Client ID quota is used up until about {0}", &[&until]),
                (LimitKind::Quota, false) => mf_i18n::tf("The personal app quota is used up until about {0}", &[&until]),
                (LimitKind::RateLimited, true) => mf_i18n::tf("Spotify slowed down the shared Client ID until {0}", &[&until]),
                (LimitKind::RateLimited, false) => mf_i18n::tf("Spotify slowed down the personal app until {0}", &[&until]),
            }
        };
        let parts: Vec<String> = [
            self.shared.as_ref().map(|limit| line(limit, true)),
            self.personal.as_ref().map(|limit| line(limit, false)),
        ]
        .into_iter()
        .flatten()
        .collect();
        if parts.is_empty() {
            return String::new();
        }
        mf_i18n::tf("{0}. Anything that goes through the Spotify session still works; the rest will wait.", &[&parts.join(". ")])
    }
}

/// Local time via `after` (HH:MM).
fn clock_after(after: Duration) -> String {
    let at = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_secs(after.as_secs() as i64 + 59))
        .unwrap_or_else(|_| jiff::Timestamp::now());
    at.to_zoned(jiff::tz::TimeZone::system()).strftime("%H:%M").to_string()
}

#[derive(Clone, Copy, Debug)]
struct Block {
    kind: LimitKind,
    since: Instant,
    until: Instant,
    probed_at: Option<Instant>,
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Go,
    Fail(LimitKind),
    Wait(Duration),
}

#[derive(Debug)]
struct SourceState {
    sent: VecDeque<Instant>,
    block: Option<Block>,
    /// 429s in a row without a successful answer between them.
    strikes: u32,
    polling_until: Option<Instant>,
}

impl SourceState {
    fn new() -> Self {
        Self { sent: VecDeque::new(), block: None, strikes: 0, polling_until: None }
    }

    fn expire(&mut self, now: Instant) {
        while self.sent.front().is_some_and(|at| now.duration_since(*at) >= WINDOW) {
            self.sent.pop_front();
        }
        if self.block.is_some_and(|block| block.until <= now) {
            self.block = None;
        }
    }

    /// Whether to let the request through now. Go records it in the window.
    fn decide(&mut self, priority: Priority, now: Instant, budget: usize) -> Decision {
        self.expire(now);
        if let Some(block) = self.block.as_mut() {
            if priority < Priority::Interactive {
                return Decision::Fail(block.kind);
            }
            let short = block.kind == LimitKind::RateLimited && block.until - now <= SHORT_HOLD;
            if !short {
                if block.probed_at.is_some_and(|at| now.duration_since(at) < block.kind.probe_every()) {
                    return Decision::Fail(block.kind);
                }
                // a probe: one request finds out whether the limit ended earlier than promised
                block.probed_at = Some(now);
            }
        }
        if priority == Priority::Polling && self.polling_until.is_some_and(|until| until > now) {
            return Decision::Fail(LimitKind::RateLimited);
        }
        let allowed = share(priority, budget);
        let used = self.sent.len();
        if used < allowed {
            self.sent.push_back(now);
            return Decision::Go;
        }
        match priority {
            Priority::Interactive => unreachable!("pages have no window limit"),
            Priority::Polling => Decision::Fail(LimitKind::RateLimited),
            // room appears when the request holding the background share leaves the window
            Priority::Background => Decision::Wait((self.sent[used - allowed] + WINDOW).saturating_duration_since(now)),
        }
    }

    fn limited(&mut self, now: Instant, retry_after: Duration, quota: bool) {
        self.strikes = self.strikes.saturating_add(1);
        let kind = if quota { LimitKind::Quota } else { LimitKind::RateLimited };
        let hold = if quota {
            retry_after.clamp(QUOTA_MIN, QUOTA_MAX)
        } else {
            retry_after.clamp(Duration::from_secs(1), RATE_LIMIT_MAX)
        };
        let until = now + hold;
        self.block = Some(match self.block {
            Some(block) => Block {
                kind: if block.kind == LimitKind::Quota { LimitKind::Quota } else { kind },
                since: block.since,
                until: block.until.max(until),
                probed_at: block.probed_at,
            },
            None => Block { kind, since: now, until, probed_at: None },
        });
        let backoff = POLL_BACKOFF
            .saturating_mul(1 << (self.strikes - 1).min(4))
            .min(POLL_BACKOFF_MAX)
            .max(hold.min(QUOTA_MAX));
        self.polling_until = Some(self.polling_until.map_or(now + backoff, |at| at.max(now + backoff)));
    }

    /// A successful answer to a request sent after the cooldown began lifts it.
    fn succeeded(&mut self, sent_at: Instant) {
        if self.block.is_some_and(|block| sent_at < block.since) {
            return;
        }
        self.block = None;
        self.strikes = 0;
        self.polling_until = None;
    }

    fn visible(&self, now: Instant) -> Option<Limit> {
        self.block
            .filter(|block| block.until > now && block.until - block.since >= SHOW_AFTER)
            .map(|block| Limit { kind: block.kind, until: block.until })
    }
}

type Key = (ApiSource, String);
/// The GET answer the coalesced requests are waiting for (None — still in flight).
type Flight = watch::Receiver<Option<Result<String, ApiError>>>;

/// Removes its own flight from the table, even if the request was dropped.
struct Landing<'a> {
    flights: &'a Mutex<HashMap<(u64, Key), Flight>>,
    key: &'a (u64, Key),
    flight: Flight,
}

impl Drop for Landing<'_> {
    fn drop(&mut self) {
        let mut flights = lock(self.flights);
        if flights.get(self.key).is_some_and(|current| current.same_channel(&self.flight)) {
            flights.remove(self.key);
        }
    }
}

struct Cached {
    at: Instant,
    ttl: Duration,
    body: String,
}

/// Shared by both sources: each has its own window and cooldown, the cache and coalescing are one.
pub struct Limits {
    shared: Mutex<SourceState>,
    personal: Mutex<SourceState>,
    cache: Mutex<HashMap<Key, Cached>>,
    flights: Mutex<HashMap<(u64, Key), Flight>>,
    /// Grows on every write and exit: answers of past generations are not cached or coalesced with new ones.
    generation: AtomicU64,
    last_write: Mutex<Option<Instant>>,
    status: watch::Sender<WebApiLimits>,
    /// Who wakes the UI when the cooldown ends.
    me: Mutex<Weak<Limits>>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            shared: Mutex::new(SourceState::new()),
            personal: Mutex::new(SourceState::new()),
            cache: Mutex::new(HashMap::new()),
            flights: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            last_write: Mutex::new(None),
            status: watch::channel(WebApiLimits::default()).0,
            me: Mutex::new(Weak::new()),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Limits {
    pub fn new() -> Arc<Self> {
        let limits = Arc::new(Self::default());
        *lock(&limits.me) = Arc::downgrade(&limits);
        limits
    }

    fn source(&self, source: ApiSource) -> std::sync::MutexGuard<'_, SourceState> {
        lock(match source {
            ApiSource::Shared => &self.shared,
            ApiSource::Personal => &self.personal,
        })
    }

    pub fn watch(&self) -> watch::Receiver<WebApiLimits> {
        self.status.subscribe()
    }

    pub fn snapshot(&self) -> WebApiLimits {
        self.status.borrow().clone()
    }

    /// Whether the source the operation would land on is cooling down right now (for background work that prefers not to even start).
    pub fn blocked(&self, source: ApiSource) -> Option<LimitKind> {
        let mut state = self.source(source);
        state.expire(Instant::now());
        state.block.map(|block| block.kind)
    }

    /// Let a request through. Background may wait for room in the window; polling and anything under cooldown get an error at once.
    pub(crate) async fn admit(&self, source: ApiSource, priority: Priority) -> Result<(), ApiError> {
        let started = Instant::now();
        loop {
            let decision = self.source(source).decide(priority, Instant::now(), budget(source));
            match decision {
                Decision::Go => return Ok(()),
                Decision::Fail(kind) => {
                    tracing::debug!("Web API source={source}: {priority:?} held back, cooldown {kind:?}");
                    return Err(kind.error());
                }
                Decision::Wait(wait) => {
                    if started.elapsed() + wait > BACKGROUND_WAIT {
                        return Err(ApiError::RateLimited);
                    }
                    tokio::time::sleep(wait.max(Duration::from_millis(50))).await;
                }
            }
        }
    }

    /// A retry of the same request (401, 5xx, short 429) is also a request in the window.
    pub(crate) fn count_retry(&self, source: ApiSource) {
        self.source(source).sent.push_back(Instant::now());
    }

    pub(crate) fn note_limited(&self, source: ApiSource, retry_after: Duration, quota: bool) {
        self.source(source).limited(Instant::now(), retry_after, quota);
        self.publish();
    }

    pub(crate) fn note_success(&self, source: ApiSource, sent_at: Instant) {
        let changed = {
            let mut state = self.source(source);
            let before = state.block.is_some();
            state.succeeded(sent_at);
            before && state.block.is_none()
        };
        if changed {
            self.publish();
        }
    }

    fn publish(&self) {
        let now = Instant::now();
        let limits = WebApiLimits {
            shared: self.source(ApiSource::Shared).visible(now),
            personal: self.source(ApiSource::Personal).visible(now),
        };
        let wake = [limits.shared, limits.personal].into_iter().flatten().map(|limit| limit.until).min();
        let changed = self.status.send_if_modified(|current| {
            if *current == limits {
                return false;
            }
            *current = limits;
            true
        });
        // when the cooldown ends, the UI must learn about it without a new request
        if changed
            && let Some(wake) = wake
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let me = lock(&self.me).clone();
            runtime.spawn(async move {
                tokio::time::sleep_until(tokio::time::Instant::from_std(wake)).await;
                if let Some(limits) = me.upgrade() {
                    limits.publish();
                }
            });
        }
    }

    // ---- GET coalescing and cache ----

    /// GET: an answer from the cache, the answer of an identical in-flight request, or a new `fetch` request.
    pub(crate) async fn get<F>(&self, source: ApiSource, path: &str, query: &[(&str, String)], fetch: F) -> Result<String, ApiError>
    where
        F: Future<Output = Result<String, ApiError>>,
    {
        let key: Key = (source, cache_key(path, query));
        let ttl = ttl_for(path);
        let generation = self.generation.load(Ordering::SeqCst);
        if !ttl.is_zero()
            && let Some(body) = self.cached(&key, ttl)
        {
            return Ok(body);
        }
        let flight_key = (generation, key);
        let joined = {
            let mut flights = lock(&self.flights);
            match flights.get(&flight_key) {
                Some(flight) => Err(flight.clone()),
                None => {
                    let (sender, flight) = watch::channel(None);
                    flights.insert(flight_key.clone(), flight.clone());
                    Ok((sender, flight))
                }
            }
        };
        let result = match joined {
            Ok((sender, flight)) => {
                let landing = Landing { flights: &self.flights, key: &flight_key, flight };
                let result = fetch.await;
                sender.send_replace(Some(result.clone()));
                drop(landing);
                result
            }
            Err(mut flight) => {
                let answer = flight.wait_for(Option::is_some).await.ok().and_then(|answer| answer.clone());
                match answer {
                    Some(answer) => answer,
                    // the leading request was dropped — we ask ourselves
                    None => fetch.await,
                }
            }
        };
        if let Ok(body) = &result
            && !ttl.is_zero()
            && self.generation.load(Ordering::SeqCst) == generation
            && (is_catalog(&flight_key.1.1) || !self.recently_written())
        {
            let mut cache = lock(&self.cache);
            if cache.len() >= CACHE_MAX {
                let now = Instant::now();
                cache.retain(|_, cached| now.duration_since(cached.at) < cached.ttl);
                if cache.len() >= CACHE_MAX {
                    cache.clear();
                }
            }
            cache.insert(flight_key.1, Cached { at: Instant::now(), ttl, body: body.clone() });
        }
        result
    }

    fn cached(&self, key: &Key, ttl: Duration) -> Option<String> {
        let cache = lock(&self.cache);
        cache.get(key).filter(|cached| cached.at.elapsed() < ttl).map(|cached| cached.body.clone())
    }

    fn recently_written(&self) -> bool {
        lock(&self.last_write).is_some_and(|at| at.elapsed() < WRITE_QUIET)
    }

    /// A write (PUT/POST/DELETE) by any source: the cache of own data and coalescing start over (the catalog stays).
    pub(crate) fn wrote(&self) {
        *lock(&self.last_write) = Some(Instant::now());
        self.forget_user_data();
    }

    /// Forget the answer cache (logout, account switch).
    pub fn forget(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        lock(&self.cache).clear();
    }

    /// Forget what a write outside the Web API may have changed (library and playlists through a session): the catalog stays.
    pub fn forget_user_data(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        lock(&self.cache).retain(|key, _| is_catalog(&key.1));
    }
}

fn cache_key(path: &str, query: &[(&str, String)]) -> String {
    let mut pairs: Vec<String> = query.iter().map(|(name, value)| format!("{name}={value}")).collect();
    pairs.sort();
    format!("{path}?{}", pairs.join("&"))
}

/// Player state cannot be cached: it changes every second. The catalog is long-lived, own lists for a minute.
fn ttl_for(path: &str) -> Duration {
    if path.starts_with("/me/player") {
        Duration::ZERO
    } else if is_catalog(path) {
        CATALOG_TTL
    } else if path.starts_with("/me/") {
        USER_TTL
    } else {
        CACHE_TTL
    }
}

/// Catalog reference data: a write to the library or a playlist does not change it.
fn is_catalog(path: &str) -> bool {
    const CATALOG: [&str; 6] = ["/albums", "/artists", "/tracks", "/shows", "/episodes", "/audiobooks"];
    CATALOG.iter().any(|prefix| path.starts_with(prefix))
}

/// A pause with +10…30 % spread: clients that got a 429 together do not come back together.
pub(crate) fn jittered(wait: Duration) -> Duration {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    wait.mul_f64(1.1 + 0.2 * f64::from(nanos % 1000) / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn at(start: Instant, secs: u64) -> Instant {
        start + Duration::from_secs(secs)
    }

    #[test]
    fn priorities_share_the_window() {
        let start = Instant::now();
        let mut state = SourceState::new();
        let budget = 40;
        // polling: an eighth of the window, beyond that a refusal without waiting
        for _ in 0..5 {
            assert_eq!(state.decide(Priority::Polling, start, budget), Decision::Go);
        }
        assert_eq!(state.decide(Priority::Polling, start, budget), Decision::Fail(LimitKind::RateLimited));
        // background: up to a third, beyond that it waits until the window frees up
        for _ in 5..13 {
            assert_eq!(state.decide(Priority::Background, start, budget), Decision::Go);
        }
        assert_eq!(state.decide(Priority::Background, at(start, 1), budget), Decision::Wait(Duration::from_secs(29)));
        // a page always gets through
        for _ in 0..30 {
            assert_eq!(state.decide(Priority::Interactive, start, budget), Decision::Go);
        }
        // everything is free again through the window
        assert_eq!(state.decide(Priority::Polling, at(start, 31), budget), Decision::Go);
    }

    #[test]
    fn rate_limit_stops_background_and_polling_but_page_probes() {
        let start = Instant::now();
        let mut state = SourceState::new();
        state.limited(start, Duration::from_secs(120), false);
        assert_eq!(state.decide(Priority::Background, start, 40), Decision::Fail(LimitKind::RateLimited));
        assert_eq!(state.decide(Priority::Polling, start, 40), Decision::Fail(LimitKind::RateLimited));
        // a page: one probe, the next no earlier than 15 s later
        assert_eq!(state.decide(Priority::Interactive, at(start, 1), 40), Decision::Go);
        assert_eq!(state.decide(Priority::Interactive, at(start, 2), 40), Decision::Fail(LimitKind::RateLimited));
        assert_eq!(state.decide(Priority::Interactive, at(start, 17), 40), Decision::Go);
        // the limit ended — background goes again, polling still backs off (≥ 120 s)
        assert_eq!(state.decide(Priority::Background, at(start, 121), 40), Decision::Go);
        assert_eq!(state.decide(Priority::Polling, at(start, 100), 40), Decision::Fail(LimitKind::RateLimited));
    }

    #[test]
    fn short_retry_after_is_left_to_the_client_for_pages() {
        let start = Instant::now();
        let mut state = SourceState::new();
        state.limited(start, Duration::from_secs(2), false);
        for secs in 0..2 {
            assert_eq!(state.decide(Priority::Interactive, at(start, secs), 40), Decision::Go);
        }
        assert_eq!(state.decide(Priority::Background, start, 40), Decision::Fail(LimitKind::RateLimited));
    }

    #[test]
    fn polling_backs_off_harder_with_each_strike() {
        let start = Instant::now();
        let mut state = SourceState::new();
        state.limited(start, Duration::from_secs(1), false);
        assert_eq!(state.decide(Priority::Polling, at(start, 59), 40), Decision::Fail(LimitKind::RateLimited));
        state.limited(start, Duration::from_secs(1), false);
        assert_eq!(state.decide(Priority::Polling, at(start, 119), 40), Decision::Fail(LimitKind::RateLimited));
        assert_eq!(state.decide(Priority::Polling, at(start, 121), 40), Decision::Go);
        state.succeeded(at(start, 121));
        state.limited(at(start, 200), Duration::from_secs(1), false);
        assert_eq!(state.decide(Priority::Polling, at(start, 261), 40), Decision::Go);
    }

    #[test]
    fn quota_holds_at_least_ten_minutes_and_only_a_later_success_lifts_it() {
        let start = Instant::now();
        let mut state = SourceState::new();
        state.limited(start, Duration::from_secs(1), true);
        assert_eq!(state.decide(Priority::Background, at(start, 300), 40), Decision::Fail(LimitKind::Quota));
        // an answer to a request sent before the quota does not lift it
        state.succeeded(start - Duration::from_secs(1));
        assert!(state.block.is_some());
        state.succeeded(at(start, 60));
        assert!(state.block.is_none());
        assert_eq!(state.decide(Priority::Background, at(start, 61), 40), Decision::Go);
    }

    #[test]
    fn only_lasting_limits_reach_the_interface() {
        let start = Instant::now();
        let mut state = SourceState::new();
        state.limited(start, Duration::from_secs(2), false);
        assert!(state.visible(start).is_none());
        state.limited(start, Duration::from_secs(300), true);
        let limit = state.visible(start).unwrap();
        assert_eq!(limit.kind, LimitKind::Quota);
        let text = WebApiLimits { shared: Some(limit), personal: None }.user_text();
        assert!(text.starts_with("The shared Client ID quota is used up"), "{text}");
        assert!(WebApiLimits::default().user_text().is_empty());
    }

    #[test]
    fn task_priority_overrides_the_path_default() {
        assert_eq!(priority_of(true, "/me/player"), Priority::Polling);
        assert_eq!(priority_of(true, "/me/player/devices"), Priority::Interactive);
        assert_eq!(priority_of(false, "/me/player"), Priority::Interactive);
        let inside = PRIORITY.sync_scope(Priority::Background, || priority_of(true, "/me/player"));
        assert_eq!(inside, Priority::Background);
    }

    /// Coalescing must not get in the way of launching requests in tokio::spawn (Send for any lifetimes).
    #[test]
    fn web_requests_stay_spawnable() {
        fn spawnable<F: Future + Send + 'static>(_: F) {}
        struct Service {
            client: crate::ApiClient,
        }
        impl Service {
            async fn albums(&self) -> Result<usize, ApiError> {
                Ok(self.client.saved_albums(0, 50).await?.items.len())
            }
        }
        let client = || {
            crate::ApiClient::new(reqwest::Client::new(), Arc::new(crate::NetActivity::default()), 20, 50, ApiSource::Shared)
        };
        let service = Arc::new(Service { client: client() });
        spawnable(async move { service.albums().await });
        let plain = client();
        spawnable(async move { plain.playback_state().await });
    }

    #[tokio::test]
    async fn a_dropped_leader_hands_the_request_to_a_follower() {
        let limits = Limits::new();
        let calls = AtomicUsize::new(0);
        let leader = limits.get(ApiSource::Shared, "/tracks/1", &[], async {
            calls.fetch_add(1, Ordering::SeqCst);
            std::future::pending::<Result<String, ApiError>>().await
        });
        let follower = limits.get(ApiSource::Shared, "/tracks/1", &[], async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ApiError>("own".to_owned())
        });
        let follower = async {
            tokio::task::yield_now().await;
            follower.await
        };
        // the leader is dropped on timeout — the waiter asks for itself
        let leader = tokio::time::timeout(Duration::from_millis(20), leader);
        let (dropped, own) = tokio::join!(leader, follower);
        assert!(dropped.is_err());
        assert_eq!(own.unwrap(), "own");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(lock(&limits.flights).is_empty());
    }

    #[tokio::test]
    async fn identical_gets_in_flight_share_one_request() {
        let limits = Limits::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let (release, wait) = tokio::sync::watch::channel(false);
        let fetch = || {
            let calls = calls.clone();
            let mut wait = wait.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                let _ = wait.wait_for(|go| *go).await;
                Ok::<_, ApiError>("body".to_owned())
            }
        };
        let query = [("limit", "50".to_owned())];
        let first = limits.get(ApiSource::Shared, "/me/albums", &query, fetch());
        let second = limits.get(ApiSource::Shared, "/me/albums", &query, fetch());
        let other = limits.get(ApiSource::Personal, "/me/albums", &query, fetch());
        let opener = async {
            tokio::task::yield_now().await;
            release.send(true).unwrap();
        };
        let (a, b, c, ()) = tokio::join!(first, second, other, opener);
        assert_eq!((a.unwrap(), b.unwrap(), c.unwrap()), ("body".into(), "body".into(), "body".into()));
        // one request for the shared source, one for the personal one
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn answers_are_cached_briefly_and_writes_drop_them() {
        let limits = Limits::new();
        let calls = AtomicUsize::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ApiError>(format!("v{}", calls.load(Ordering::SeqCst)))
        };
        assert_eq!(limits.get(ApiSource::Shared, "/me/albums", &[], fetch()).await.unwrap(), "v1");
        assert_eq!(limits.get(ApiSource::Shared, "/me/albums", &[], fetch()).await.unwrap(), "v1");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(limits.get(ApiSource::Shared, "/albums/1", &[], fetch()).await.unwrap(), "v2");
        limits.wrote();
        assert_eq!(limits.get(ApiSource::Shared, "/me/albums", &[], fetch()).await.unwrap(), "v3");
        // right after a write own data is not cached: rechecks see a fresh answer
        assert_eq!(limits.get(ApiSource::Shared, "/me/albums", &[], fetch()).await.unwrap(), "v4");
        // a write does not change the catalog: it stays in the cache
        assert_eq!(limits.get(ApiSource::Shared, "/albums/1", &[], fetch()).await.unwrap(), "v2");
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn player_state_and_errors_are_never_cached() {
        let limits = Limits::new();
        let calls = AtomicUsize::new(0);
        let ok = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ApiError>(String::new())
        };
        limits.get(ApiSource::Shared, "/me/player", &[], ok()).await.unwrap();
        limits.get(ApiSource::Shared, "/me/player", &[], ok()).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let failing = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(ApiError::RateLimited)
        };
        assert!(limits.get(ApiSource::Shared, "/artists/1", &[], failing()).await.is_err());
        assert!(limits.get(ApiSource::Shared, "/artists/1", &[], failing()).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn background_fails_fast_while_the_source_cools_down() {
        let limits = Limits::new();
        let mut status = limits.watch();
        limits.note_limited(ApiSource::Shared, Duration::from_secs(600), false);
        assert!(status.has_changed().unwrap());
        assert!(status.borrow_and_update().shared.is_some());
        let started = Instant::now();
        let refused = background(async { limits.admit(ApiSource::Shared, priority_of(true, "/me/albums")).await }).await;
        assert!(matches!(refused, Err(ApiError::RateLimited)));
        assert!(started.elapsed() < Duration::from_millis(100));
        // the other source is not affected
        assert!(background(limits.admit(ApiSource::Personal, Priority::Background)).await.is_ok());
        limits.note_success(ApiSource::Shared, Instant::now());
        assert!(status.borrow_and_update().shared.is_none());
    }
}
