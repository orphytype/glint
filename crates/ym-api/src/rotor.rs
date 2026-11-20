//! Rotor: "My Wave" sessions (what the current clients use) and radio stations.
//!
//! A session is started from seeds (`"user:onyourwave"`, `"settingMoodEnergy:calm"`,
//! `"track:123"`, `"artist:45"` ...) and then fed the ids of the tracks already played to get the
//! next batch. Feedback events teach the Wave what was liked or skipped.

use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::client::{as_id, iso8601, seg, unix_millis};
use crate::models::Track;
use crate::{Client, Result, WaveEvent};

/// Where feedback says playback came from.
const FROM: &str = "desktop_win-radio-web-glint";

/// One running Wave session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RotorSession {
    pub id: String,
    pub batch_id: Option<String>,
    pub tracks: Vec<Track>,
    /// Seeds the service accepted, e.g. `["user:onyourwave", "settingDiversity:discover"]`.
    pub accepted_seeds: Vec<String>,
    /// A one-line description of what the Wave is playing ("Calm, unfamiliar").
    pub description: Option<String>,
    /// The service ended the session (nothing more to play for these seeds).
    pub terminated: bool,
}

/// A radio station from the dashboard or the full list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Station {
    /// `"type:tag"`, e.g. `"genre:rock"`.
    pub id: String,
    pub name: String,
    /// `"#RRGGBB"` background colour of the station's icon.
    pub color: Option<String>,
    /// Image URL template of the station's icon.
    pub icon: Option<String>,
}

/// The three knobs of "My Wave".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WaveSettings {
    /// `"default"`, `"favorite"`, `"discover"`, `"popular"`.
    pub diversity: &'static str,
    /// `"all"`, `"active"`, `"fun"`, `"calm"`, `"sad"`.
    pub mood_energy: &'static str,
    /// `"any"`, `"russian"`, `"not-russian"`, `"without-words"`.
    pub language: &'static str,
}

impl WaveSettings {
    /// The session seeds these settings stand for, after the base station.
    pub fn seeds(&self) -> Vec<String> {
        let mut out = vec!["user:onyourwave".to_owned()];
        if !self.diversity.is_empty() && self.diversity != "default" {
            out.push(format!("settingDiversity:{}", self.diversity));
        }
        if !self.mood_energy.is_empty() && self.mood_energy != "all" {
            out.push(format!("settingMoodEnergy:{}", self.mood_energy));
        }
        if !self.language.is_empty() && self.language != "any" {
            out.push(format!("settingLanguage:{}", self.language));
        }
        out
    }
}

impl Client {
    /// Starts a Wave session from `seeds`. `queue` are track ids already playing, so the
    /// first batch continues from them.
    pub async fn rotor_session_new(&self, seeds: &[String], queue: &[String]) -> Result<RotorSession> {
        let mut body = json!({
            "seeds": seeds,
            "includeTracksInResponse": true,
            "includeWaveModel": true,
            "interactive": true,
        });
        if !queue.is_empty() {
            body["queue"] = json!(queue);
        }
        let v = self.call(self.api(Method::POST, "/rotor/session/new").json(&body)).await?;
        Ok(parse_session(v, None))
    }

    /// The next batch of a running session. `queue` is what has been played (most recent last).
    pub async fn rotor_session_tracks(&self, session: &str, queue: &[String]) -> Result<RotorSession> {
        let body = json!({ "queue": queue });
        let v = self.call(self.api(Method::POST, &format!("/rotor/session/{}/tracks", seg(session)?)).json(&body)).await?;
        Ok(parse_session(v, Some(session)))
    }

    /// Reports what happened in a session. Best-effort.
    pub async fn rotor_session_feedback(&self, session: &str, batch_id: Option<&str>, event: WaveEvent, track_id: Option<&str>, played_secs: Option<f64>) -> Result<()> {
        let mut ev = Map::new();
        ev.insert("type".into(), json!(event.as_str()));
        ev.insert("timestamp".into(), json!(iso8601(unix_millis())));
        if let Some(t) = track_id {
            ev.insert("trackId".into(), json!(t));
        }
        if let Some(s) = played_secs {
            ev.insert("totalPlayedSeconds".into(), json!(s));
        }
        let mut body = json!({ "event": Value::Object(ev), "from": FROM });
        if let Some(b) = batch_id {
            body["batchId"] = json!(b);
        }
        self.call(self.api(Method::POST, &format!("/rotor/session/{}/feedback", seg(session)?)).json(&body)).await.map(drop)
    }

    /// Stations picked for the account.
    pub async fn stations_dashboard(&self) -> Result<Vec<Station>> {
        let v = self.get("/rotor/stations/dashboard", &[]).await?;
        Ok(parse_stations(v.get("stations").unwrap_or(&Value::Null)))
    }

    /// Every station there is.
    pub async fn stations(&self) -> Result<Vec<Station>> {
        let v = self.get("/rotor/stations/list", &[]).await?;
        Ok(parse_stations(&v))
    }

    /// Saves the "My Wave" knobs on the account, so other devices pick them up.
    pub async fn set_wave_settings(&self, settings: WaveSettings) -> Result<()> {
        let body = json!({
            "moodEnergy": if settings.mood_energy.is_empty() { "all" } else { settings.mood_energy },
            "diversity": if settings.diversity.is_empty() { "default" } else { settings.diversity },
            "language": if settings.language.is_empty() { "any" } else { settings.language },
            "type": "rotor",
        });
        self.call(self.api(Method::POST, "/rotor/station/user:onyourwave/settings3").json(&body)).await.map(drop)
    }
}

pub(crate) fn parse_session(mut v: Value, known_id: Option<&str>) -> RotorSession {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned).filter(|s| !s.is_empty());
    let id = text(&v, "radioSessionId").or_else(|| known_id.map(str::to_owned)).unwrap_or_default();
    let batch_id = text(&v, "batchId");
    let description =
        v.pointer("/descriptionSeed/value").and_then(Value::as_str).map(str::to_owned).or_else(|| v.pointer("/wave/description").and_then(Value::as_str).map(str::to_owned));
    let accepted_seeds = v
        .get("acceptedSeeds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let ty = s.get("type").and_then(Value::as_str)?;
            let val = s.get("value").map(as_id).filter(|x| !x.is_empty())?;
            Some(format!("{ty}:{val}"))
        })
        .collect();
    let terminated = v.get("terminated").and_then(Value::as_bool).unwrap_or(false);
    let tracks = match v.get_mut("sequence").map(Value::take) {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|mut item| item.get_mut("track").map(Value::take))
            .filter_map(|t| serde_json::from_value::<Track>(t).ok())
            .filter(|t| !t.id.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    RotorSession { id, batch_id, tracks, accepted_seeds, description, terminated }
}

fn parse_stations(v: &Value) -> Vec<Station> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let st = item.get("station").unwrap_or(item);
            let ty = st.pointer("/id/type").and_then(Value::as_str)?;
            let tag = st.pointer("/id/tag").and_then(Value::as_str)?;
            let name = st.get("name").and_then(Value::as_str).unwrap_or(tag).to_owned();
            let color = st.pointer("/icon/backgroundColor").and_then(Value::as_str).map(str::to_owned);
            let icon = st.pointer("/icon/imageUrl").and_then(Value::as_str).map(str::to_owned);
            Some(Station { id: format!("{ty}:{tag}"), name, color, icon })
        })
        .collect()
}
