//! Getting an OAuth token.
//!
//! The user signs in on Yandex's own page in their own browser; this app never sees a
//! password. Yandex redirects to an address that carries the token in its fragment
//! (`…#access_token=<token>&token_type=bearer&expires_in=…`); the user pastes that address
//! (or just the token) back, and [`extract_token`] fishes the token out of it.

/// The public OAuth client id of the official desktop client, which every open-source client
/// of the service uses as well. It identifies an application, it is not a secret.
pub const OAUTH_CLIENT_ID: &str = "23cabbbdc6cd418abb4b39c32c41195d";

/// The page where the user grants access.
pub fn authorize_url() -> String {
    format!("https://oauth.yandex.ru/authorize?response_type=token&client_id={OAUTH_CLIENT_ID}")
}

fn plausible(t: &str) -> Option<String> {
    let t = t.trim();
    let ok = (20..=512).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    ok.then(|| t.to_owned())
}

/// Pulls a token out of whatever was pasted: the bare token, `OAuth <token>`, a whole
/// redirect address, or a query/fragment string containing `access_token=`.
pub fn extract_token(pasted: &str) -> Option<String> {
    let s = pasted.trim().trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '<' | '>')).trim();
    const KEY: &str = "access_token=";
    if let Some(i) = s.find(KEY) {
        let rest = &s[i + KEY.len()..];
        let end = rest.find(['&', '#', ' ', '"', '\'', '\r', '\n']).unwrap_or(rest.len());
        return plausible(&rest[..end]);
    }
    let bare = s.strip_prefix("OAuth ").or_else(|| s.strip_prefix("oauth ")).or_else(|| s.strip_prefix("Bearer ")).unwrap_or(s);
    plausible(bare)
}
