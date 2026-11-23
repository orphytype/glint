//! Library management: the account's own playlists (create, rename, edit tracks), play reporting
//! and the server-side play queue.
//!
//! Track edits are diffs against a playlist `revision`; the service refuses a stale revision, so
//! each call returns the updated playlist whose revision the next edit must use.

use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::{from_value, iso8601, seg, unix_millis};
use crate::models::{Playlist, Track};
use crate::{Client, Error, Result};

/// Who can see a playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
}

impl Visibility {
    fn as_str(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
        }
    }
}

/// A play to report, so history, "recently played" and the artist's stats count it.
#[derive(Debug, Clone, Default)]
pub struct PlayReport {
    pub track_id: String,
    pub album_id: Option<String>,
    pub playlist_id: Option<String>,
    /// Where it was played from, e.g. `"desktop_win-own_tracks-track-default"`.
    pub from: String,
    pub play_id: String,
    pub track_length_secs: f64,
    pub total_played_secs: f64,
    pub end_position_secs: f64,
}

/// A server-side queue (what other devices see as "playing on ...").
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueInfo {
    pub id: String,
    pub modified: Option<String>,
}

impl Client {
    pub async fn playlist_create(&self, uid: i64, title: &str, visibility: Visibility) -> Result<Playlist> {
        let v = self.post_form(&format!("/users/{uid}/playlists/create"), &[("title", title), ("visibility", visibility.as_str())]).await?;
        from_value(v, "playlist")
    }

    pub async fn playlist_delete(&self, uid: i64, kind: i64) -> Result<()> {
        self.post_form(&format!("/users/{uid}/playlists/{kind}/delete"), &[]).await.map(drop)
    }

    pub async fn playlist_rename(&self, uid: i64, kind: i64, title: &str) -> Result<Playlist> {
        let v = self.post_form(&format!("/users/{uid}/playlists/{kind}/name"), &[("value", title)]).await?;
        from_value(v, "playlist")
    }

    pub async fn playlist_set_description(&self, uid: i64, kind: i64, text: &str) -> Result<Playlist> {
        let v = self.post_form(&format!("/users/{uid}/playlists/{kind}/description"), &[("value", text)]).await?;
        from_value(v, "playlist")
    }

    pub async fn playlist_set_visibility(&self, uid: i64, kind: i64, visibility: Visibility) -> Result<Playlist> {
        let v = self.post_form(&format!("/users/{uid}/playlists/{kind}/visibility"), &[("value", visibility.as_str())]).await?;
        from_value(v, "playlist")
    }

    /// Inserts tracks (`(track id, album id)`) at `at`.
    pub async fn playlist_insert(&self, uid: i64, kind: i64, revision: i64, at: usize, tracks: &[(String, String)]) -> Result<Playlist> {
        let mut list = Vec::with_capacity(tracks.len());
        for (id, album) in tracks {
            if album.is_empty() {
                list.push(json!({ "id": seg(id)? }));
            } else {
                list.push(json!({ "id": seg(id)?, "albumId": seg(album)? }));
            }
        }
        self.playlist_change(uid, kind, revision, json!([{ "op": "insert", "at": at, "tracks": list }])).await
    }

    /// Moves a track from `from` to `to` (`track` is `(track id, album id)`).
    pub async fn playlist_move(&self, uid: i64, kind: i64, revision: i64, from: usize, to: usize, track: &(String, String)) -> Result<Playlist> {
        if from == to {
            return self.playlist(uid, kind).await;
        }
        let item = if track.1.is_empty() {
            json!({ "id": seg(&track.0)? })
        } else {
            json!({ "id": seg(&track.0)?, "albumId": seg(&track.1)? })
        };
        let diff = json!([
            { "op": "delete", "from": from, "to": from + 1, "tracks": [item.clone()] },
            { "op": "insert", "at": to, "tracks": [item] },
        ]);
        self.playlist_change(uid, kind, revision, diff).await
    }

    /// Uploads a user audio file to a playlist via the UGC loader endpoint and returns the
    /// assigned `ugc-track-id` when accepted by the service.
    pub async fn playlist_upload_ugc(&self, uid: i64, kind: i64, filename: &str, audio: &[u8]) -> Result<String> {
        let kind_s = kind.to_string();
        let meta = self
            .post_form(
                &format!("/handlers/ugc-upload.jsx?uid={uid}"),
                &[("filename", filename), ("kind", &kind_s), ("visibility", "private")],
            )
            .await?;
        let post_target = meta
            .get("post-target")
            .or_else(|| meta.get("postTarget"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Parse("missing post-target".into()))?;
        let track_id = meta
            .get("ugc-track-id")
            .or_else(|| meta.get("ugcTrackId"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Parse("missing ugc-track-id".into()))?
            .to_owned();
        let boundary = format!("----GlintUgcBoundary{}", unix_millis());
        let safe_name: String = filename.chars().filter(|c| *c != '"' && *c != '\r' && *c != '\n').collect();
        let mut body = Vec::with_capacity(audio.len() + 256);
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(audio);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let resp = self
            .inner
            .http
            .post(post_target)
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .body(body)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(Error::Api(resp.text().await.unwrap_or_default()));
        }
        Ok(track_id)
    }

    /// Removes the tracks at positions `from..to`.
    pub async fn playlist_remove(&self, uid: i64, kind: i64, revision: i64, from: usize, to: usize) -> Result<Playlist> {
        if to <= from {
            return Err(Error::Parse("empty range".into()));
        }
        self.playlist_change(uid, kind, revision, json!([{ "op": "delete", "from": from, "to": to }])).await
    }

    async fn playlist_change(&self, uid: i64, kind: i64, revision: i64, diff: Value) -> Result<Playlist> {
        let (kind_s, rev, diff) = (kind.to_string(), revision.to_string(), diff.to_string());
        let v = self.post_form(&format!("/users/{uid}/playlists/{kind}/change"), &[("kind", &kind_s), ("revision", &rev), ("diff", &diff)]).await?;
        from_value(v, "playlist")
    }

    /// Reports a finished (or abandoned) play. Best-effort.
    pub async fn play_audio(&self, uid: i64, report: &PlayReport) -> Result<()> {
        let now = iso8601(unix_millis());
        let (len, total, end) = (format!("{:.3}", report.track_length_secs), format!("{:.3}", report.total_played_secs), format!("{:.3}", report.end_position_secs));
        let uid = uid.to_string();
        let mut form: Vec<(&str, &str)> = vec![
            ("track-id", seg(&report.track_id)?),
            ("from-cache", "false"),
            ("from", if report.from.is_empty() { "desktop_win-glint" } else { &report.from }),
            ("play-id", &report.play_id),
            ("uid", &uid),
            ("timestamp", &now),
            ("track-length-seconds", &len),
            ("total-played-seconds", &total),
            ("end-position-seconds", &end),
            ("client-now", &now),
        ];
        if let Some(a) = report.album_id.as_deref() {
            form.push(("album-id", a));
        }
        if let Some(p) = report.playlist_id.as_deref() {
            form.push(("playlist-id", p));
        }
        self.post_form("/play-audio", &form).await.map(drop)
    }

    /// The account's server-side queues, newest first.
    pub async fn queues(&self, device: &str) -> Result<Vec<QueueInfo>> {
        let mut v = self.call(self.api(Method::GET, "/queues").header("X-Yandex-Music-Device", device)).await?;
        Ok(crate::models::lossy_vec(v.get_mut("queues").map(Value::take).unwrap_or(Value::Null)))
    }

    /// Publishes the local queue, so other devices (and "continue listening") can see it.
    /// Returns the queue's id.
    pub async fn queue_create(&self, device: &str, tracks: &[Track], current: usize, context_type: &str, context_id: &str) -> Result<String> {
        let items: Vec<Value> =
            tracks.iter().map(|t| json!({ "trackId": t.id, "albumId": t.album().map(|a| a.id.clone()).unwrap_or_default(), "from": "desktop_win-glint" })).collect();
        let body = json!({
            "context": { "type": context_type, "id": context_id, "description": "" },
            "tracks": items,
            "currentIndex": current,
            "isInteractive": true,
        });
        let v = self.call(self.api(Method::POST, "/queues").header("X-Yandex-Music-Device", device).json(&body)).await?;
        Ok(v.get("id").and_then(Value::as_str).unwrap_or_default().to_owned())
    }

    pub async fn queue_set_position(&self, device: &str, queue_id: &str, index: usize) -> Result<()> {
        let idx = index.to_string();
        let rb = self
            .api(Method::POST, &format!("/queues/{}/update-position", seg(queue_id)?))
            .header("X-Yandex-Music-Device", device)
            .query(&[("currentIndex", idx.as_str()), ("isInteractive", "false")]);
        self.call(rb).await.map(drop)
    }
}

/// The `X-Yandex-Music-Device` header for this app.
pub fn device_header(device_id: &str) -> String {
    format!("os=Windows; os_version=10; manufacturer=Glint; model=Desktop; clid=; device_id={device_id}; uuid={device_id}")
}
