use std::fmt;

/// What can go wrong with a call.
#[derive(Debug)]
pub enum Error {
    /// The API refused, with its stable code and a message for a person.
    /// `status` is the HTTP status. `retry_after` is set on `rate_limited`.
    Api {
        status: u16,
        code: String,
        message: String,
        request_id: Option<String>,
        retry_after: Option<u64>,
    },
    /// The request did not reach the service, or the connection broke.
    Transport(String),
    /// The answer was not what the API documents. The service is
    /// unversioned and only adds fields, so this points at a proxy or a
    /// different server at the base URL.
    Decode(String),
    /// The token or the base URL is not usable as given.
    Config(String),
}

impl Error {
    /// The API's stable code, if the API refused.
    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Api { code, .. } => Some(code),
            _ => None,
        }
    }

    /// True for `unauthorized`: the token is wrong, expired or revoked.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Error::Api { status: 401, .. })
    }

    /// True for `not_found`.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::Api { status: 404, .. })
    }

    /// True when the call may be repeated as it was after a pause: the
    /// connection failed, the per-minute rate limit was reached, or the
    /// service had a passing problem. Not for the daily and monthly
    /// sending limits, which last until the day or month is over, and
    /// not for `unavailable`, which means the service at the base URL
    /// does not offer the call at all.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Api {
                status: 429, code, ..
            } => code != "daily_limit" && code != "monthly_limit",
            Error::Api { status, code, .. } => *status >= 500 && *status != 501 && code != "unavailable",
            Error::Transport(_) => true,
            _ => false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Api {
                code,
                message,
                status,
                ..
            } => write!(f, "{message} ({code}, HTTP {status})"),
            Error::Transport(m) => write!(f, "could not reach EMX: {m}"),
            Error::Decode(m) => write!(f, "unexpected answer from EMX: {m}"),
            Error::Config(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Decode(e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Transport(e.to_string())
    }
}
