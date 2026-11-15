//! Events/holidays data source with an on-disk cache.
//!
//! The calendar core is fully local; the pnldev API is only used for holidays,
//! occasions (`event`) and the lunar date (`moon`). Rules implemented here:
//!
//! * month-level requests only (`?year=Y&month=M`), never per-day;
//! * 5 second timeout and a dedicated User-Agent;
//! * disk cache in `app_cache_dir()/events/<year>-<month>.json`;
//! * past months are cached forever, current/future months expire after 7 days;
//! * stale-while-revalidate: show the cached value immediately, refresh in the
//!   background and emit `events-updated` when the fresh data lands;
//! * exponential retry backoff (5 min → 30 min), no busy loop;
//! * after a successful fetch, prefetch the neighbouring months one at a time.
//!
//! The HTTP source is behind the [`EventsProvider`] trait so it can be swapped.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::jalaali;

pub const BASE_URL: &str = "https://pnldev.com/api/calender";
pub const USER_AGENT: &str = "PersianCalendar/1.0 (+https://github.com/DaneshJoy/PersianCalendar; Windows)";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CACHE_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
const DAILY_REFRESH_SECONDS: i64 = 24 * 60 * 60;
const BACKOFF_MIN: Duration = Duration::from_secs(5 * 60);
const BACKOFF_MAX: Duration = Duration::from_secs(30 * 60);
/// Schema version of the on-disk cache. Bump it whenever the shape of
/// [`DayInfo`] changes so legacy caches (which stored events as plain strings)
/// are discarded and re-fetched instead of being misread.
const CACHE_VERSION: u32 = 2;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CivilDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LunarDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

/// Lunar month/day parsed from an occasion's bracketed date hint.
///
/// `"شهادت حضرت علی علیه السلام [ ٢١ رمضان ]"` becomes `{ month: 9, day: 21 }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LunarMonthDay {
    pub month: u32,
    pub day: u32,
}

/// One occasion from the API, plus the lunar date it belongs to.
///
/// `lunar` is `Some` for occasions whose display day must follow the
/// `lunarOffset` setting (Ramadan, Ashura, …) and `None` for fixed ones
/// (Gregorian/Jalali occasions, national days, plain strings).
///
/// Field order matters for the JSON contract: `text` first, then `lunar`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Occasion {
    pub text: String,
    pub lunar: Option<LunarMonthDay>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DayInfo {
    pub holiday: bool,
    pub events: Vec<Occasion>,
    pub moon: Option<LunarDate>,
    pub gregorian: Option<CivilDate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthData {
    pub year: i32,
    pub month: u32,
    pub fetched_at: i64,
    pub days: BTreeMap<u32, DayInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DataSource {
    Cache,
    Network,
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthEvents {
    pub year: i32,
    pub month: u32,
    pub source: DataSource,
    pub stale: bool,
    pub days: BTreeMap<u32, DayInfo>,
    pub error: Option<String>,
}

impl MonthEvents {
    fn empty(year: i32, month: u32, error: Option<String>) -> Self {
        Self {
            year,
            month,
            source: DataSource::None,
            stale: false,
            days: BTreeMap::new(),
            error,
        }
    }

    fn from_cache(year: i32, month: u32, cached: MonthData, stale: bool) -> Self {
        Self {
            year,
            month,
            source: DataSource::Cache,
            stale,
            days: cached.days,
            error: None,
        }
    }

    fn from_network(year: i32, month: u32, data: MonthData) -> Self {
        Self {
            year,
            month,
            source: DataSource::Network,
            stale: false,
            days: data.days,
            error: None,
        }
    }
}

// ---------------------------------------------------------------------------
// API parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApiEnvelope {
    status: bool,
    result: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ApiSolar {
    #[serde(default)]
    day: u32,
    #[serde(default)]
    month: u32,
    #[serde(default)]
    year: i32,
}

#[derive(Debug, Deserialize)]
struct ApiDay {
    #[serde(default)]
    solar: Option<ApiSolar>,
    #[serde(default)]
    moon: Option<ApiLunar>,
    #[serde(default)]
    gregorian: Option<ApiGregorian>,
    #[serde(default)]
    holiday: bool,
    #[serde(default)]
    event: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiLunar {
    day: u32,
    month: u32,
    year: i32,
}

#[derive(Debug, Deserialize)]
struct ApiGregorian {
    day: u32,
    month: u32,
    year: i32,
}

/// Strip trailing lunar/Gregorian date hints from an event string.
///
/// The API often returns strings like `"شهادت حضرت علی علیه السلام [ ٢١ رمضان ]"`
/// or `"روز جهانی زنان [ 8 March ]"`; everything inside square brackets is
/// dropped, along with collapsed whitespace and dangling separators.
pub fn clean_event(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '[' | '【' => depth += 1,
            ']' | '】' => {
                if depth > 0 {
                    depth -= 1;
                } else {
                    cleaned.push(ch);
                }
            }
            _ => {
                if depth == 0 {
                    cleaned.push(ch);
                }
            }
        }
    }
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .trim()
        .trim_end_matches([',', '،', '؛', '-', '|'])
        .trim()
        .to_string()
}

/// First bracketed segment of `raw`, e.g. `" ٢١ رمضان "` for
/// `"شهادت حضرت علی علیه السلام [ ٢١ رمضان ]"`. Nested brackets are handled
/// with a depth counter; unmatched openers yield `None`.
fn bracket_hint(raw: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in raw.char_indices() {
        match ch {
            '[' | '【' => {
                if depth == 0 {
                    start = index + ch.len_utf8();
                }
                depth += 1;
            }
            ']' | '】' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&raw[start..index]);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Lunar month spellings, 1-based, in the normalised form produced by
/// [`normalize_hint`] (no spaces/ZWNJ, Persian ی/ک/ه and bare ا).
const LUNAR_MONTHS: [&[&str]; 12] = [
    &["محرم", "محرمالحرام"],
    &["صفر", "صفرالمظفر"],
    &["ربیعالاول", "ربیعالاولی"],
    &["ربیعالثانی", "ربیعالثانیه", "ربیعالاخر", "ربیعالاخره"],
    &["جمادیالاول", "جمادیالاولی"],
    &["جمادیالثانی", "جمادیالثانیه", "جمادیالاخر", "جمادیالاخره"],
    &["رجب", "رجبالمرجب"],
    &["شعبان", "شعبانالمعظم"],
    &["رمضان", "رمضانالمبارک"],
    &["شوال", "شوالالمکرم"],
    &["ذیالقعده", "ذوالقعده", "ذیقعده"],
    &["ذیالحجه", "ذوالحجه", "ذیحجه"],
];

/// Normalise a bracketed hint for comparison: ASCII digits, no diacritics,
/// no tatweel/ZWNJ/whitespace, Persian letter shapes.
fn normalize_hint(hint: &str) -> String {
    let mut normalized = String::with_capacity(hint.len());
    for ch in hint.chars() {
        // Arabic-Indic (٠-٩) and Persian (۰-۹) digits become ASCII.
        if ('\u{0660}'..='\u{0669}').contains(&ch) {
            if let Some(digit) = char::from_u32('0' as u32 + (ch as u32 - '\u{0660}' as u32)) {
                normalized.push(digit);
            }
            continue;
        }
        if ('\u{06F0}'..='\u{06F9}').contains(&ch) {
            if let Some(digit) = char::from_u32('0' as u32 + (ch as u32 - '\u{06F0}' as u32)) {
                normalized.push(digit);
            }
            continue;
        }
        match ch {
            // Harakat and tatweel are noise for a month lookup.
            '\u{064B}'..='\u{0652}' | '\u{0670}' | '\u{0640}' => {}
            // Invisible joiners (the API mixes them into month names).
            '\u{200C}' | '\u{200D}' => {}
            // Arabic letter variants collapse onto their Persian shapes.
            'ي' => normalized.push('ی'),
            'ك' => normalized.push('ک'),
            'ة' | 'ۀ' => normalized.push('ه'),
            'أ' | 'إ' | 'آ' => normalized.push('ا'),
            'ؤ' => normalized.push('و'),
            'ئ' => normalized.push('ی'),
            _ if ch.is_whitespace() => {}
            _ => normalized.extend(ch.to_lowercase()),
        }
    }
    normalized
}

/// Resolve a normalised month name (with the leading day digits removed and
/// any trailing year digits trimmed) to a 1-based lunar month number. Longer
/// spellings win, so `"ربیعالاولی"` is preferred over `"ربیعالاول"`.
fn lunar_month(remainder: &str) -> Option<u32> {
    let mut best: Option<(usize, u32)> = None;
    for (index, variants) in LUNAR_MONTHS.iter().enumerate() {
        for variant in variants.iter() {
            if remainder == *variant || remainder.starts_with(*variant) {
                let length = variant.chars().count();
                if best.map_or(true, |(best_length, _)| length > best_length) {
                    best = Some((length, index as u32 + 1));
                }
            }
        }
    }
    best.map(|(_, month)| month)
}

/// Parse a bracketed hint such as `" ٢١ رمضان "` into a lunar month/day.
///
/// `None` covers Gregorian/Jalali hints (`" 8 March "`, `" 1367 خورشیدی "`),
/// hints without a day ("Jalali month names") and anything unrecognised.
fn lunar_from_hint(hint: &str) -> Option<LunarMonthDay> {
    let normalized = normalize_hint(hint);
    let digits: String = normalized
        .chars()
        .take(2)
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    let day: u32 = digits.parse().ok()?;
    if !(1..=30).contains(&day) {
        return None;
    }
    let remainder = &normalized[digits.len()..];
    // A trailing Hijri year (`"٢١ رمضان ١٤٤٨"`) is noise for the month lookup.
    let remainder = remainder.trim_end_matches(|ch: char| ch.is_ascii_digit());
    let month = lunar_month(remainder)?;
    Some(LunarMonthDay { month, day })
}

/// Classify one raw API `event` string.
///
/// The displayed text never contains the bracketed date hint (see
/// [`clean_event`]); the hint itself is parsed so the caller knows whether the
/// occasion is lunar (moves with `lunarOffset`) or fixed.
pub fn parse_occasion(raw: &str) -> Occasion {
    Occasion {
        text: clean_event(raw),
        lunar: bracket_hint(raw).and_then(lunar_from_hint),
    }
}

fn parse_month(
    year: i32,
    month: u32,
    result: &serde_json::Value,
    fetched_at: i64,
) -> Result<MonthData, String> {
    let object = result
        .as_object()
        .ok_or_else(|| "unexpected API payload: result is not an object".to_string())?;

    let mut days: BTreeMap<u32, DayInfo> = BTreeMap::new();
    for (key, value) in object {
        // Monthly payloads are keyed by day number; ignore anything else.
        let Ok(day_number) = key.parse::<u32>() else {
            continue;
        };
        if !(1..=31).contains(&day_number) {
            continue;
        }
        // Annual payloads nest months under the top level; skip those neatly.
        if value.get("solar").is_none() {
            continue;
        }
        let Ok(api_day) = serde_json::from_value::<ApiDay>(value.clone()) else {
            continue;
        };
        let events: Vec<Occasion> = api_day
            .event
            .iter()
            .map(|event| parse_occasion(event))
            .filter(|occasion| !occasion.text.is_empty())
            .collect();
        days.insert(
            day_number,
            DayInfo {
                holiday: api_day.holiday,
                events,
                moon: api_day.moon.map(|moon| LunarDate {
                    year: moon.year,
                    month: moon.month,
                    day: moon.day,
                }),
                gregorian: api_day.gregorian.map(|gregorian| CivilDate {
                    year: gregorian.year,
                    month: gregorian.month,
                    day: gregorian.day,
                }),
            },
        );
    }

    if days.is_empty() {
        return Err("unexpected API payload: no day entries in result".to_string());
    }

    Ok(MonthData {
        year,
        month,
        fetched_at,
        days,
    })
}

/// Parse an API response body (exposed for unit tests).
pub fn parse_response(
    year: i32,
    month: u32,
    body: &str,
    fetched_at: i64,
) -> Result<MonthData, String> {
    let envelope: ApiEnvelope =
        serde_json::from_str(body).map_err(|err| format!("invalid JSON: {err}"))?;
    if !envelope.status {
        return Err("API reported status=false".to_string());
    }
    parse_month(year, month, &envelope.result, fetched_at)
}

// ---------------------------------------------------------------------------
// Provider abstraction
// ---------------------------------------------------------------------------

pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Plugging point for the events source. A different endpoint can be added by
/// implementing this trait and passing it to [`EventsState::with_provider`].
pub trait EventsProvider: Send + Sync {
    fn fetch_month<'a>(&'a self, year: i32, month: u32)
        -> BoxFuture<'a, Result<MonthData, String>>;
}

pub struct PnldevProvider {
    client: reqwest::Client,
    base_url: &'static str,
}

impl PnldevProvider {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            base_url: BASE_URL,
        }
    }
}

impl EventsProvider for PnldevProvider {
    fn fetch_month<'a>(
        &'a self,
        year: i32,
        month: u32,
    ) -> BoxFuture<'a, Result<MonthData, String>> {
        Box::pin(async move {
            // Only `year` and `month` are ever sent; no user data, no identifiers.
            let url = format!("{}?year={}&month={}", self.base_url, year, month);
            let response = self
                .client
                .get(url)
                .send()
                .await
                .map_err(|err| format!("network error: {err}"))?;
            if !response.status().is_success() {
                return Err(format!("HTTP {}", response.status()));
            }
            let body = response
                .text()
                .await
                .map_err(|err| format!("failed to read response: {err}"))?;
            parse_response(year, month, &body, chrono::Utc::now().timestamp())
        })
    }
}

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        .gzip(true)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

// ---------------------------------------------------------------------------
// Cache + state
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    /// Cache schema version; see [`CACHE_VERSION`].
    #[serde(default)]
    version: u32,
    fetched_at: i64,
    year: i32,
    month: u32,
    days: BTreeMap<u32, DayInfo>,
}

struct BackoffState {
    failures: u32,
    next_allowed: Option<Instant>,
}

impl Default for BackoffState {
    fn default() -> Self {
        Self {
            failures: 0,
            next_allowed: None,
        }
    }
}

pub struct EventsState {
    client: reqwest::Client,
    cache_dir: PathBuf,
    provider: Arc<dyn EventsProvider>,
    backoff: Mutex<BackoffState>,
    in_flight: Mutex<HashSet<(i32, u32)>>,
    prefetch_gate: tokio::sync::Mutex<()>,
}

impl EventsState {
    pub fn new(cache_dir: PathBuf) -> Self {
        let client = build_http_client();
        let provider: Arc<dyn EventsProvider> = Arc::new(PnldevProvider::new(client.clone()));
        Self {
            client,
            cache_dir,
            provider,
            backoff: Mutex::new(BackoffState::default()),
            in_flight: Mutex::new(HashSet::new()),
            prefetch_gate: tokio::sync::Mutex::new(()),
        }
    }

    #[allow(dead_code)]
    pub fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    fn events_dir(&self) -> PathBuf {
        self.cache_dir.join("events")
    }

    fn cache_path(&self, year: i32, month: u32) -> PathBuf {
        self.events_dir().join(format!("{year}-{month}.json"))
    }

    fn read_cache(&self, year: i32, month: u32) -> Option<MonthData> {
        let path = self.cache_path(year, month);
        let bytes = std::fs::read(path).ok()?;
        match serde_json::from_slice::<CacheFile>(&bytes) {
            // Legacy caches stored events as plain strings, so the schema
            // version must match before the data is trusted.
            Ok(file) if file.version != CACHE_VERSION => {
                eprintln!(
                    "[events] ignoring cache file with version {} (want {})",
                    file.version, CACHE_VERSION
                );
                None
            }
            Ok(file) => Some(MonthData {
                year: file.year,
                month: file.month,
                fetched_at: file.fetched_at,
                days: file.days,
            }),
            Err(err) => {
                eprintln!("[events] ignoring corrupt cache file: {err}");
                None
            }
        }
    }

    fn write_cache(&self, data: &MonthData) {
        let dir = self.events_dir();
        if let Err(err) = std::fs::create_dir_all(&dir) {
            eprintln!("[events] cannot create cache dir: {err}");
            return;
        }
        let file = CacheFile {
            version: CACHE_VERSION,
            fetched_at: data.fetched_at,
            year: data.year,
            month: data.month,
            days: data.days.clone(),
        };
        let path = self.cache_path(data.year, data.month);
        // Write to a temporary file first so a crash never leaves half a file.
        let temporary = path.with_extension("json.tmp");
        match serde_json::to_vec(&file) {
            Ok(bytes) => {
                if let Err(err) = std::fs::write(&temporary, bytes) {
                    eprintln!("[events] cannot write cache: {err}");
                    return;
                }
                if let Err(err) = std::fs::rename(&temporary, &path) {
                    eprintln!("[events] cannot finalise cache file: {err}");
                    let _ = std::fs::remove_file(&temporary);
                }
            }
            Err(err) => eprintln!("[events] cannot serialise cache: {err}"),
        }
    }

    /// True when cached data for this month must be re-fetched.
    fn is_expired(&self, year: i32, month: u32, fetched_at: i64) -> bool {
        let (today, _) = jalaali::today_local();
        let is_past = year < today.year || (year == today.year && month < today.month);
        if is_past {
            return false; // past months never expire
        }
        let now = chrono::Utc::now().timestamp();
        now.saturating_sub(fetched_at) > CACHE_TTL_SECONDS
    }

    fn backoff_allows(&self) -> bool {
        let state = self.backoff.lock().unwrap();
        match state.next_allowed {
            Some(next) => Instant::now() >= next,
            None => true,
        }
    }

    fn record_failure(&self) {
        let mut state = self.backoff.lock().unwrap();
        state.failures = state.failures.saturating_add(1);
        let factor = 1u32 << (state.failures.min(3) - 1);
        let delay = (BACKOFF_MIN * factor).min(BACKOFF_MAX);
        state.next_allowed = Some(Instant::now() + delay);
    }

    fn record_success(&self) {
        let mut state = self.backoff.lock().unwrap();
        state.failures = 0;
        state.next_allowed = None;
    }

    /// Return cached month data without touching the network.
    pub fn cached_month(&self, year: i32, month: u32) -> Option<MonthData> {
        self.read_cache(year, month)
    }

    /// Cached info for one day, if available.
    pub fn cached_day(&self, year: i32, month: u32, day: u32) -> Option<DayInfo> {
        self.read_cache(year, month)
            .and_then(|data| data.days.get(&day).cloned())
    }

    /// True when the current month cache is older than a day (daily refresh).
    pub fn needs_daily_refresh(&self, year: i32, month: u32) -> bool {
        match self.read_cache(year, month) {
            Some(data) => {
                let now = chrono::Utc::now().timestamp();
                now.saturating_sub(data.fetched_at) > DAILY_REFRESH_SECONDS
            }
            None => true,
        }
    }

    /// Get a month of events, using cache first and refreshing when needed.
    ///
    /// * `force_refresh` – bypass the cache when deciding to fetch, but still
    ///   honour the retry backoff and the "one in-flight request per month" rule.
    pub async fn get_month(
        &self,
        app: &AppHandle,
        year: i32,
        month: u32,
        force_refresh: bool,
    ) -> MonthEvents {
        let cached = self.read_cache(year, month);

        let stale = match &cached {
            Some(data) => self.is_expired(year, month, data.fetched_at),
            None => true,
        };

        // A usable, fresh cache entry is returned as-is.
        if let Some(data) = &cached {
            if !force_refresh && !stale {
                return MonthEvents::from_cache(year, month, data.clone(), false);
            }
        }

        // Fresh cache is served; an expired cache is served immediately too,
        // while a background task refreshes it (stale-while-revalidate).
        if let Some(data) = &cached {
            let clone = data.clone();
            let state_needs_refresh = force_refresh || stale;
            if state_needs_refresh && self.backoff_allows() {
                self.spawn_refresh(app.clone(), year, month);
            }
            return MonthEvents::from_cache(year, month, clone, stale);
        }

        if !self.backoff_allows() {
            return MonthEvents::empty(
                year,
                month,
                Some("در انتظار تلاش مجدد؛ اتصال شبکه را بررسی کنید".to_string()),
            );
        }

        match self.fetch_and_store(year, month).await {
            Ok(data) => {
                self.record_success();
                self.emit_updated(app, year, month);
                self.spawn_prefetch_neighbours(app.clone(), year, month);
                MonthEvents::from_network(year, month, data)
            }
            Err(err) => {
                self.record_failure();
                eprintln!("[events] fetch {year}-{month} failed: {err}");
                MonthEvents::empty(year, month, Some(err))
            }
        }
    }

    async fn fetch_and_store(&self, year: i32, month: u32) -> Result<MonthData, String> {
        let data = self.provider.fetch_month(year, month).await?;
        self.write_cache(&data);
        Ok(data)
    }

    fn spawn_refresh(&self, app: AppHandle, year: i32, month: u32) {
        {
            let mut in_flight = self.in_flight.lock().unwrap();
            if !in_flight.insert((year, month)) {
                return; // already refreshing this month
            }
        }
        let state = app.state::<crate::AppState>();
        let events = state.events.clone();
        drop(state);
        tauri::async_runtime::spawn(async move {
            if events.backoff_allows() {
                match events.fetch_and_store(year, month).await {
                    Ok(_) => {
                        events.record_success();
                        events.emit_updated(&app, year, month);
                        events.spawn_prefetch_neighbours(app.clone(), year, month);
                    }
                    Err(err) => {
                        events.record_failure();
                        eprintln!("[events] background refresh {year}-{month} failed: {err}");
                    }
                }
            }
            events.in_flight.lock().unwrap().remove(&(year, month));
        });
    }

    fn spawn_prefetch_neighbours(&self, app: AppHandle, year: i32, month: u32) {
        let state = app.state::<crate::AppState>();
        let events = state.events.clone();
        drop(state);
        tauri::async_runtime::spawn(async move {
            for (neighbour_year, neighbour_month) in neighbours(year, month) {
                events
                    .prefetch_if_needed(&app, neighbour_year, neighbour_month)
                    .await;
            }
        });
    }

    /// Fetch a month into the cache if it is missing or expired (polite prefetch).
    pub async fn prefetch_if_needed(&self, app: &AppHandle, year: i32, month: u32) {
        if let Some(cached) = self.read_cache(year, month) {
            if !self.is_expired(year, month, cached.fetched_at) {
                return;
            }
        }
        if !self.backoff_allows() {
            return;
        }
        // Concurrency 1: prefetches never compete with the user-facing request.
        let _gate = self.prefetch_gate.lock().await;
        if let Some(cached) = self.read_cache(year, month) {
            if !self.is_expired(year, month, cached.fetched_at) {
                return;
            }
        }
        match self.fetch_and_store(year, month).await {
            Ok(_) => {
                self.record_success();
                self.emit_updated(app, year, month);
            }
            Err(err) => {
                self.record_failure();
                eprintln!("[events] prefetch {year}-{month} failed: {err}");
            }
        }
    }

    fn emit_updated(&self, app: &AppHandle, year: i32, month: u32) {
        let _ = app.emit(
            "events-updated",
            serde_json::json!({ "year": year, "month": month }),
        );
    }

    /// Delete the whole events cache directory.
    pub fn clear_cache(&self) -> Result<(), String> {
        let dir = self.events_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|err| format!("cannot clear cache: {err}"))?;
        }
        Ok(())
    }
}

fn neighbours(year: i32, month: u32) -> Vec<(i32, u32)> {
    let mut result = Vec::with_capacity(2);
    if month > 1 {
        result.push((year, month - 1));
    }
    if month < 12 {
        result.push((year, month + 1));
    }
    result
}

/// Path of the events cache directory for diagnostics/logging.
pub fn events_cache_dir(app_cache_dir: &Path) -> PathBuf {
    app_cache_dir.join("events")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bracketed_dates() {
        assert_eq!(
            clean_event("شهادت حضرت علی علیه السلام [ ٢١ رمضان ]"),
            "شهادت حضرت علی علیه السلام"
        );
        assert_eq!(clean_event("روز جهانی زنان [ 8 March ]"), "روز جهانی زنان");
        assert_eq!(clean_event("تعطیل رسمی"), "تعطیل رسمی");
        assert_eq!(clean_event("[ 1 Farvardin ]"), "");
        assert_eq!(
            clean_event("عید نوروز [ ١ فروردین ] [ Nowruz ]"),
            "عید نوروز"
        );
    }

    #[test]
    fn parses_monthly_payload() {
        let body = r#"{
            "status": true,
            "result": {
                "1": {
                    "solar": {"day": 1, "month": 8, "year": 1405, "dayWeek": "ج"},
                    "moon": {"day": 11, "month": 5, "year": 1448},
                    "gregorian": {"day": 23, "month": 10, "year": 2026},
                    "holiday": true,
                    "event": ["روز آمار", "بزرگداشت [ ١١ جمادی‌الاول ]"]
                },
                "2": {
                    "solar": {"day": 2, "month": 8, "year": 1405, "dayWeek": "ش"},
                    "moon": {"day": 12, "month": 5, "year": 1448},
                    "gregorian": {"day": 24, "month": 10, "year": 2026},
                    "holiday": false,
                    "event": []
                }
            }
        }"#;
        let data = parse_response(1405, 8, body, 1234).expect("payload parses");
        assert_eq!(data.days.len(), 2);
        let first = data.days.get(&1).unwrap();
        assert!(first.holiday);
        assert_eq!(first.events.len(), 2);
        assert_eq!(first.events[0].text, "روز آمار");
        assert_eq!(first.events[0].lunar, None);
        assert_eq!(first.events[1].text, "بزرگداشت");
        assert_eq!(
            first.events[1].lunar,
            Some(LunarMonthDay { month: 5, day: 11 })
        );
        assert_eq!(first.moon.unwrap().month, 5);
        assert!(data.days.get(&2).unwrap().events.is_empty());
    }

    #[test]
    fn classifies_real_api_occasions() {
        let lunar_cases = [
            ("شهادت حضرت علی علیه السلام [ ٢١ رمضان ]", 9, 21),
            ("شهادت حضرت فاطمه سلام الله علیها [ ٣ جمادي الثانيه ]", 6, 3),
            (
                "ولادت حضرت قائم عجل الله تعالی فرجه و جشن نیمه شعبان [ ۱۵ شعبان ]",
                8,
                15,
            ),
            ("عید سعید فطر [ ١ شوال ]", 10, 1),
            ("تاسوعای حسینی [ ۹ محرم ]", 1, 9),
        ];
        for (raw, month, day) in lunar_cases {
            let occasion = parse_occasion(raw);
            assert_eq!(
                occasion.lunar,
                Some(LunarMonthDay { month, day }),
                "lunar classification failed for {raw}"
            );
            // The hint is still stripped from the displayed text.
            assert!(!occasion.text.contains('['), "text not cleaned for {raw}");
        }

        for raw in [
            "روز جهانی زنان [ 8 March ]",
            "شلیک به پرواز 655 ایران ایر توسط ناو وینسنس [ 1367 خورشیدی ]",
            "جشن نوروز/جشن سال نو",
        ] {
            assert_eq!(parse_occasion(raw).lunar, None, "expected fixed: {raw}");
        }
        assert_eq!(
            parse_occasion("جشن نوروز/جشن سال نو").text,
            "جشن نوروز/جشن سال نو"
        );
    }

    #[test]
    fn recognises_every_lunar_month_spelling() {
        let cases = [
            ("١ محرم", 1),
            ("٢ محرم الحرام", 1),
            ("٣ صفر", 2),
            ("٤ صفر المظفر", 2),
            ("٥ ربيع الاول", 3),
            ("٦ ربیع الاولی", 3),
            ("٧ ربيع الثاني", 4),
            ("٨ ربیع الثانیه", 4),
            ("٩ ربيع الآخر", 4),
            ("١٠ جمادي الاول", 5),
            ("١١ جمادي الاولي", 5),
            ("١٢ جمادي الثانيه", 6),
            ("١٣ جمادی الآخره", 6),
            ("١٤ رجب", 7),
            ("١٥ شعبان", 8),
            ("١٦ رمضان", 9),
            ("١٧ رمضان المبارک", 9),
            ("١٨ شوال", 10),
            ("١٩ ذوالقعده", 11),
            ("٢٠ ذی الحجه", 12),
            ("٢١ ذو الحجه", 12),
            ("٢٢ ذیحجه", 12),
        ];
        for (hint, expected) in cases {
            let raw = format!("مناسبت [ {hint} ]");
            let occasion = parse_occasion(&raw);
            assert_eq!(
                occasion.lunar.map(|lunar| lunar.month),
                Some(expected),
                "wrong month for {raw}"
            );
        }
    }

    #[test]
    fn uses_the_first_bracketed_hint_with_depth_handling() {
        let occasion = parse_occasion("عید نوروز [ ١ فروردین ] [ Nowruz ]");
        assert_eq!(occasion.lunar, None);
        assert_eq!(occasion.text, "عید نوروز");

        let occasion = parse_occasion("مناسبت [ ١١ رمضان [ ١٤٤٨ ] ]");
        assert_eq!(occasion.lunar, Some(LunarMonthDay { month: 9, day: 11 }));
        assert_eq!(occasion.text, "مناسبت");

        // A day outside 1..=30 is not a valid lunar date.
        assert_eq!(parse_occasion("مناسبت [ ٣١ رمضان ]").lunar, None);
        assert_eq!(parse_occasion("مناسبت [ ٠ رمضان ]").lunar, None);
    }

    #[test]
    fn serializes_occasions_in_the_contract_shape() {
        let fixed = Occasion {
            text: "روز آمار".to_string(),
            lunar: None,
        };
        let lunar = Occasion {
            text: "شهادت حضرت فاطمه".to_string(),
            lunar: Some(LunarMonthDay { month: 6, day: 3 }),
        };
        // Field order `text`, `lunar` and `null` for fixed occasions are part
        // of the frontend contract.
        assert_eq!(
            serde_json::to_string(&fixed).unwrap(),
            r#"{"text":"روز آمار","lunar":null}"#
        );
        assert_eq!(
            serde_json::to_string(&lunar).unwrap(),
            r#"{"text":"شهادت حضرت فاطمه","lunar":{"month":6,"day":3}}"#
        );
        let parsed: Occasion =
            serde_json::from_str(r#"{"text":"x","lunar":{"month":1,"day":9}}"#).unwrap();
        assert_eq!(parsed.lunar, Some(LunarMonthDay { month: 1, day: 9 }));
    }

    #[test]
    fn ignores_cache_files_from_an_older_schema() {
        use std::collections::BTreeMap;

        let dir =
            std::env::temp_dir().join(format!("persiancalendar-events-cache-{}", std::process::id()));
        let state = EventsState::new(dir.clone());
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(state.events_dir()).unwrap();

        // A structurally current cache with the old version number is stale.
        let legacy = r#"{"version":1,"fetched_at":0,"year":1405,"month":8,"days":{"1":{"holiday":false,"events":[{"text":"x","lunar":null}],"moon":null,"gregorian":null}}}"#;
        std::fs::write(state.cache_path(1405, 8), legacy).unwrap();
        assert!(state.read_cache(1405, 8).is_none());

        // A cache written by this build round-trips.
        let data = MonthData {
            year: 1405,
            month: 8,
            fetched_at: 42,
            days: BTreeMap::new(),
        };
        state.write_cache(&data);
        let cached = state
            .read_cache(1405, 8)
            .expect("current cache is readable");
        assert_eq!(cached.fetched_at, 42);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_payloads() {
        assert!(parse_response(1405, 8, "not json", 0).is_err());
        assert!(parse_response(1405, 8, r#"{"status": false, "result": {}}"#, 0).is_err());
        assert!(parse_response(1405, 8, r#"{"status": true, "result": {}}"#, 0).is_err());
        assert!(parse_response(1405, 8, r#"{"status": true, "result": []}"#, 0).is_err());
    }

    #[test]
    fn computes_neighbours() {
        assert_eq!(neighbours(1405, 1), vec![(1405, 2)]);
        assert_eq!(neighbours(1405, 8), vec![(1405, 7), (1405, 9)]);
        assert_eq!(neighbours(1405, 12), vec![(1405, 11)]);
    }
}
