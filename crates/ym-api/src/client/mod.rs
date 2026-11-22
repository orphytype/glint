//! The HTTP client. Every network call the app makes lives under `client/`, so "what does this
//! app send where" is answered in one module:
//!
//! * the OAuth token is attached to requests for `api.music.yandex.net` (see [`Client::api`])
//!   and to one request to Yandex's own sign-in service, `login.yandex.ru/info`, which says which
//!   picture the account uses (see [`Client::avatar`]); audio, lyrics, artwork, LRCLIB and
//!   lrc.red requests never carry it;
//! * lrc.red is asked only when the person has given a contact for its User-Agent (its terms
//!   require one); the track's title and artist go in the search query, nothing else does;
//! * redirects are followed only to Yandex hosts, `lrclib.net`, `lrc.red` and
//!   `raw.githubusercontent.com`, and URLs that came out of a response are checked against
//!   [`cover::is_trusted_host`] before they are fetched;
//! * bodies are size-limited, ids that end up in a URL path are validated;
//! * responses are parsed leniently: the service omits fields freely, so one odd element never
//!   costs the whole page.

mod catalog;
mod home;
mod lyrics;
mod media;
mod parse;
mod types;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use reqwest::header::{ACCEPT_LANGUAGE, AUTHORIZATION, HeaderValue, USER_AGENT};
use reqwest::{Method, RequestBuilder};
use serde_json::Value;

use crate::lrc::Lyrics;
use crate::lrcred;
use crate::{Error, Result, cover};

pub use types::{Likeable, Quality, SearchKind, StreamInfo, WaveEvent};

pub(crate) use parse::{as_id, from_value, https, iso8601, seg, unix_millis};

const API: &str = "https://api.music.yandex.net";
/// What the service expects its desktop client to call itself; every open-source client sends it.
const CLIENT_ID: &str = "YandexMusicDesktopAppWindows/5.29.1";
/// What the signed file-info endpoint expects to be talking to.
pub(crate) const FILE_INFO_CLIENT: &str = "YandexMusicDesktopAppWindows/5.95.0";
pub(super) const APP_UA: &str = concat!("Glint/", env!("CARGO_PKG_VERSION"), " (unofficial client)");

const MAX_JSON_BYTES: u64 = 64 << 20;
pub(super) const MAX_AUDIO_BYTES: u64 = 4 << 30;
pub(super) const MAX_IMAGE_BYTES: u64 = 16 << 20;
pub(super) const MAX_TEXT_BYTES: u64 = 4 << 20;
/// How many ids one batch request carries.
pub(super) const BATCH: usize = 100;
/// How many lrc.red answers to remember.
pub(super) const LRCRED_SEEN_MAX: usize = 64;

pub(super) struct Inner {
    pub(super) http: reqwest::Client,
    /// Pre-built sensitive `OAuth <token>` header so API requests avoid formatting/parsing per call.
    auth_header: RwLock<Option<HeaderValue>>,
    lang: RwLock<String>,
    /// The User-Agent for lrc.red; `None` keeps lrc.red switched off.
    pub(super) lrcred_ua: RwLock<Option<String>>,
    /// What lrc.red said for a track id this session, so a track is asked about once.
    pub(super) lrcred_seen: Mutex<HashMap<String, Option<Lyrics>>>,
}

/// Cheap to clone; clones share the connection pool, the token and the language.
#[derive(Clone)]
pub struct Client {
    pub(super) inner: Arc<Inner>,
}

impl Client {
    pub fn new() -> Result<Client> {
        // rustls needs its crypto chosen once per process (Windows uses SChannel instead).
        #[cfg(not(windows))]
        {
            static PROVIDER: std::sync::Once = std::sync::Once::new();
            PROVIDER.call_once(|| {
                let _ = rustls::crypto::ring::default_provider().install_default();
            });
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let next = attempt.url();
                let trusted = cover::is_trusted_host(next.as_str()) || matches!(next.host_str(), Some("lrclib.net" | "lrc.red" | "raw.githubusercontent.com"));
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if trusted {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()?;
        Ok(Client {
            inner: Arc::new(Inner { http, auth_header: RwLock::new(None), lang: RwLock::new("ru".into()), lrcred_ua: RwLock::new(None), lrcred_seen: Mutex::new(HashMap::new()) }),
        })
    }

    /// Sets (or clears) the OAuth token. Pass it through [`crate::auth::extract_token`] first.
    pub fn set_token(&self, token: Option<String>) {
        let header = token.filter(|t| !t.is_empty()).and_then(|t| {
            let mut value = HeaderValue::from_str(&format!("OAuth {t}")).ok()?;
            value.set_sensitive(true);
            Some(value)
        });
        *self.inner.auth_header.write().unwrap_or_else(PoisonError::into_inner) = header;
    }

    pub fn has_token(&self) -> bool {
        self.inner.auth_header.read().unwrap_or_else(PoisonError::into_inner).is_some()
    }

    /// `"ru"` or `"en"`: the language the service writes playlist titles and the like in.
    pub fn set_language(&self, lang: &str) {
        *self.inner.lang.write().unwrap_or_else(PoisonError::into_inner) = lang.to_owned();
    }

    /// Turns word-by-word lyrics from lrc.red on or off. Its terms ask every client to send a
    /// User-Agent with a way to reach whoever runs it, so it stays off until `contact` (a link, an
    /// address) is given; an unusable contact switches it off too.
    pub fn set_lrcred_contact(&self, contact: Option<&str>) {
        let ua = contact.and_then(|c| lrcred::user_agent(env!("CARGO_PKG_VERSION"), c));
        *self.inner.lrcred_ua.write().unwrap_or_else(PoisonError::into_inner) = ua;
        self.inner.lrcred_seen.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    pub fn lrcred_enabled(&self) -> bool {
        self.inner.lrcred_ua.read().unwrap_or_else(PoisonError::into_inner).is_some()
    }

    pub(super) fn auth_header(&self) -> Option<HeaderValue> {
        self.inner.auth_header.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// A request to the music API: the only place the token is attached.
    pub(crate) fn api(&self, method: Method, path: &str) -> RequestBuilder {
        let lang = self.inner.lang.read().unwrap_or_else(PoisonError::into_inner).clone();
        let mut rb = self
            .inner
            .http
            .request(method, format!("{API}{path}"))
            .header(USER_AGENT, CLIENT_ID)
            .header("X-Yandex-Music-Client", CLIENT_ID)
            .header(ACCEPT_LANGUAGE, lang)
            .timeout(Duration::from_secs(25));
        if let Some(value) = self.auth_header() {
            rb = rb.header(AUTHORIZATION, value);
        }
        rb
    }

    async fn call_once(&self, rb: RequestBuilder) -> Result<Value> {
        let resp = rb.send().await?;
        let status = resp.status().as_u16();
        let body = read_limited(resp, MAX_JSON_BYTES, |_, _| {}).await?;
        parse::parse_envelope(status, &String::from_utf8_lossy(&body))
    }

    /// Sends the request and returns the envelope's `result`; transient failures get one retry.
    pub(crate) async fn call(&self, rb: RequestBuilder) -> Result<Value> {
        let again = rb.try_clone();
        match self.call_once(rb).await {
            Err(e) if e.is_transient() => match again {
                Some(rb) => {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    self.call_once(rb).await
                }
                None => Err(e),
            },
            other => other,
        }
    }

    pub(crate) async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let rb = self.api(Method::GET, path);
        self.call(if query.is_empty() { rb } else { rb.query(query) }).await
    }

    pub(crate) async fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Result<Value> {
        self.call(self.api(Method::POST, path).form(form)).await
    }

    /// A plain GET of a (checked) absolute URL, without credentials. When `timeout_secs` is `None`,
    /// the transfer relies on the client's connect and per-chunk read timeouts so multi-hour audio
    /// downloads are not cut short while bytes keep arriving.
    pub(super) async fn fetch(&self, url: &str, max: u64, timeout_secs: Option<u64>, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>> {
        let mut req = self.inner.http.get(url).header(USER_AGENT, CLIENT_ID);
        if let Some(secs) = timeout_secs {
            req = req.timeout(Duration::from_secs(secs));
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Error::Http { status: status.as_u16(), body: String::new() });
        }
        read_limited(resp, max, &mut progress).await
    }

    pub(crate) async fn fetch_text(&self, url: &str) -> Result<String> {
        let bytes = self.fetch(url, MAX_TEXT_BYTES, Some(20), |_, _| {}).await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

pub(super) async fn read_limited(mut resp: reqwest::Response, max: u64, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>> {
    let total = resp.content_length();
    if total.is_some_and(|t| t > max) {
        return Err(Error::Api("response is too large".into()));
    }
    let mut buf = Vec::with_capacity(total.unwrap_or(8 << 10).min(32 << 20) as usize);
    while let Some(chunk) = resp.chunk().await? {
        if buf.len() as u64 + chunk.len() as u64 > max {
            return Err(Error::Api("response is too large".into()));
        }
        buf.extend_from_slice(&chunk);
        progress(buf.len() as u64, total);
    }
    Ok(buf)
}
