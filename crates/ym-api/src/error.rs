use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    /// Transport failure: DNS, TLS, timeout, connection reset.
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    /// The server answered with a non-success status.
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    /// 401: the token is missing, expired or revoked.
    #[error("not signed in (token missing or expired)")]
    Unauthorized,
    /// The track or the whole service is not available from this region.
    #[error("not available in your region")]
    RegionRestricted,
    /// The API answered 200 but with an `error` object, or without a `result`.
    #[error("API error: {0}")]
    Api(String),
    /// A response did not look like we expected.
    #[error("unexpected response: {0}")]
    Parse(String),
}

impl Error {
    /// Worth one more try: a timeout, a dropped connection, a 429 or a 5xx.
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Network(e) => e.is_timeout() || e.is_connect(),
            Error::Http { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }

    /// Short, human text for a toast.
    pub fn brief(&self) -> String {
        match self {
            Error::Network(e) if e.is_timeout() => "Request timed out".into(),
            Error::Network(_) => "Network error".into(),
            Error::Http { status, .. } => format!("Server error ({status})"),
            Error::Unauthorized => "Please sign in again".into(),
            Error::RegionRestricted => "Not available in your region".into(),
            Error::Api(m) | Error::Parse(m) => m.clone(),
        }
    }
}
