//! Parsing, validation and time helpers shared across the HTTP client modules.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::models::strip_nulls;
use crate::{Error, Result};

/// Ids that end up inside a URL path must be plain.
pub(crate) fn seg(id: &str) -> Result<&str> {
    let plain = !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':'));
    if plain { Ok(id) } else { Err(Error::Parse(format!("unexpected id {id:?}"))) }
}

pub(super) fn id_pairs<'a>(key: &'static str, ids: &'a [String]) -> Result<Vec<(&'static str, &'a str)>> {
    ids.iter().map(|id| seg(id).map(|id| (key, id))).collect()
}

pub(crate) fn https(url: &str) -> String {
    match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None if url.starts_with("//") => format!("https:{url}"),
        None => url.to_owned(),
    }
}

pub(super) fn as_i64(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

pub(crate) fn as_id(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub(crate) fn from_value<T: DeserializeOwned>(v: Value, what: &str) -> Result<T> {
    serde_json::from_value(v).map_err(|e| Error::Parse(format!("{what}: {e}")))
}

/// List items that may or may not be wrapped (`[{"album": {..}, "timestamp": ".."}, ..]` vs
/// `[{..}, ..]`), sorted newest-added first whenever ISO-8601 `"timestamp"` fields are present.
pub(super) fn unwrap_items_newest_first<T: DeserializeOwned>(v: Value, key: &str) -> Vec<T> {
    let Value::Array(items) = v else { return Vec::new() };
    let mut stamped: Vec<(Option<String>, T)> = items
        .into_iter()
        .filter_map(|mut item| {
            let ts = item.get("timestamp").and_then(Value::as_str).map(str::to_owned);
            let inner = item.get_mut(key).filter(|x| x.is_object()).map(Value::take);
            let parsed = serde_json::from_value(inner.unwrap_or(item)).ok()?;
            Some((ts, parsed))
        })
        .collect();
    if stamped.iter().any(|(ts, _)| ts.is_some()) {
        stamped.sort_by(|a, b| b.0.cmp(&a.0));
    }
    stamped.into_iter().map(|(_, v)| v).collect()
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_owned(),
    }
}

/// Unwraps the `{"result": …, "error": …}` envelope, mapping failures to [`Error`]s.
pub(super) fn parse_envelope(status: u16, body: &str) -> Result<Value> {
    let mut parsed: Option<Value> = serde_json::from_str(body).ok();
    let (name, message) = match parsed.as_ref().and_then(|v| v.get("error")).filter(|e| !e.is_null()) {
        Some(e) => {
            let get = |k: &str| e.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
            // Some failures put a bare string here.
            if let Some(s) = e.as_str() { (String::new(), s.to_owned()) } else { (get("name").to_lowercase(), get("message")) }
        }
        None => (String::new(), String::new()),
    };
    let has_error = !name.is_empty() || !message.is_empty();

    let authy = ["session-expired", "unauthorized", "not-auth", "no-auth", "oauth", "token"].iter().any(|k| name.contains(k));
    if status == 401 || authy {
        return Err(Error::Unauthorized);
    }
    if status == 451 || ["region", "geo", "country"].iter().any(|k| name.contains(k)) {
        return Err(Error::RegionRestricted);
    }
    if !(200..300).contains(&status) {
        let detail = if message.is_empty() { body } else { message.as_str() };
        return Err(Error::Http { status, body: truncate(detail.trim(), 200) });
    }
    if has_error {
        return Err(Error::Api(if message.is_empty() { name } else { message }));
    }
    let mut v = parsed.take().ok_or_else(|| Error::Parse("the response is not JSON".into()))?;
    match v.get_mut("result") {
        Some(result) => {
            let mut result = result.take();
            strip_nulls(&mut result);
            Ok(result)
        }
        None => Err(Error::Api("the response has no result".into())),
    }
}

/// `"2023-11-14T22:13:20.000Z"` for a Unix time in milliseconds.
pub(crate) fn iso8601(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, unix_ms % 1000)
}

pub(crate) fn unix_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
