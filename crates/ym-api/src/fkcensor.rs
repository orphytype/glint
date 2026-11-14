//! FkCensorData replacements used by the optional clean-track substitution.

use std::collections::HashMap;

use serde_json::Value;

const LIST_URL: &str = "https://raw.githubusercontent.com/fcensor-data/FkCensorData/refs/heads/main/list_v2.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    pub url: String,
    pub duration_ms: Option<u64>,
}

/// Parses the public FkCensorData list into track-id to audio-url replacements.
pub fn parse_list(json: &str) -> Option<HashMap<String, Replacement>> {
    let root: Value = serde_json::from_str(json).ok()?;
    let sources = root.get("sources")?.as_array()?;
    let mut storages = Vec::new();
    for source in sources {
        if let Some(list) = source.get("tracksStorages").and_then(Value::as_array) {
            for storage in list.iter().rev() {
                storages.push(storage);
            }
        }
    }
    let mut out = HashMap::new();
    for storage in storages {
        let template = storage.get("urlTemplate").and_then(Value::as_str);
        if let Some(tracks) = storage.get("tracks").and_then(Value::as_object) {
            for (id, value) in tracks {
                if out.contains_key(id) {
                    continue;
                }
                let (url, duration_ms) = storage_url(template, value);
                if let Some(url) = url {
                    out.insert(id.clone(), Replacement { url, duration_ms });
                }
            }
        }
        if let Some(ids) = storage.get("trackIds").and_then(Value::as_array) {
            let Some(template) = template else { continue };
            for item in ids {
                let Some(id) = item.get("id").and_then(value_id).or_else(|| value_id(item)) else { continue };
                if out.contains_key(&id) {
                    continue;
                }
                let Some(url) = normalize_url(&template.replace("%%", &id)) else { continue };
                let duration_ms = item.get("durationMs").and_then(Value::as_u64);
                out.insert(id, Replacement { url, duration_ms });
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn storage_url(template: Option<&str>, value: &Value) -> (Option<String>, Option<u64>) {
    let (raw, duration_ms) = if let Some(object) = value.as_object() {
        (object.get("url").and_then(Value::as_str), object.get("durationMs").and_then(Value::as_u64))
    } else {
        (value.as_str(), None)
    };
    let raw = match raw {
        Some(raw) => raw,
        None if value.as_u64().is_some() => "",
        None => return (None, None),
    };
    let url = if raw.is_empty() {
        let Some(id) = value.as_u64() else { return (None, None) };
        let Some(template) = template else { return (None, None) };
        template.replace("%%", &id.to_string())
    } else if raw.contains("://") {
        raw.to_owned()
    } else {
        let Some(template) = template else { return (None, None) };
        template.replace("%%", raw)
    };
    (normalize_url(&url), duration_ms)
}

fn value_id(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned).or_else(|| value.as_u64().map(|id| id.to_string()))
}

fn normalize_url(url: &str) -> Option<String> {
    let url = if url.starts_with("https://") { url.to_owned() } else { format!("https://{url}") };
    (url.strip_prefix("https://")?.split('/').next()? == "raw.githubusercontent.com").then_some(url)
}

pub async fn fetch_list(http: &reqwest::Client) -> crate::Result<HashMap<String, Replacement>> {
    let response = http.get(LIST_URL).send().await?.error_for_status()?;
    let body = response.text().await?;
    parse_list(&body).ok_or_else(|| crate::Error::Parse("invalid FkCensorData list".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_storage_templates_and_keeps_newest_entries() {
        let json = r#"{"sources":[{"tracksStorages":[
            {"urlTemplate":"raw.githubusercontent.com/fcensor-data/FkCensorData_01/refs/heads/main/tracks/%%","trackIds":[{"id":123,"durationMs":456}]},
            {"urlTemplate":"raw.githubusercontent.com/fcensor-data/FkCensorData_02/refs/heads/main/tracks/%%","tracks":{"123":{"url":"https://raw.githubusercontent.com/fcensor-data/custom/123","durationMs":789}},"trackIds":[{"id":456}]}
        ]}]}"#;
        let map = parse_list(json).unwrap();
        assert_eq!(map["123"].duration_ms, Some(789));
        assert_eq!(map["123"].url, "https://raw.githubusercontent.com/fcensor-data/custom/123");
        assert_eq!(map["456"].url, "https://raw.githubusercontent.com/fcensor-data/FkCensorData_02/refs/heads/main/tracks/456");
        assert!(parse_list(r#"{"sources":[{"tracksStorages":[{"trackIds":[{"id":123},"bad"]}]}]}"#).is_none());
        assert!(parse_list(r#"{"sources":[{"tracksStorages":[{"tracks":{"1":"https://example.com/a"}}]}]}"#).is_none());
    }
}
