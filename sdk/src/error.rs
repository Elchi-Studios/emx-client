// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fmt;

/// What can go wrong with a call.
#[derive(Debug)]
pub enum Error {
    /// The API refused, with its stable code and a message for a person.
    /// `status` is the HTTP status. `retry_after` is the pause in seconds
    /// the service asked for, which every 429 and 503 carries.
    /// `replayed` is set when a send with an `Idempotency-Key` the
    /// service had seen got the answer the first request with it got
    /// (`Idempotent-Replayed: true`): the same refusal again, and
    /// nothing sent this time either.
    Api {
        status: u16,
        code: String,
        message: String,
        request_id: Option<String>,
        retry_after: Option<u64>,
        replayed: bool,
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

    /// The pause in seconds the service asked for before the call is
    /// made again, if it named one.
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Error::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// True for `unauthorized`: the token is wrong, expired or revoked.
    /// Only a 401 is: a `503 try_later` while the service checks the
    /// token is a fault on its side, and the same token works again.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Error::Api { status: 401, .. })
    }

    /// True for `not_found`.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::Api { status: 404, .. })
    }

    /// True for `503 try_later`: a passing fault on the service's side,
    /// such as its database not answering while it checks the token, or
    /// the virus scanner being away while a message with files is sent.
    /// Nothing was done; the same call works again after `retry_after`.
    pub fn is_try_later(&self) -> bool {
        matches!(self, Error::Api { status: 503, code, .. } if code == "try_later")
    }

    /// True when the call may be repeated as it was after a pause: the
    /// connection failed, the per-minute rate limit (`rate_limited`) was
    /// reached, or the service had a passing problem (`try_later` and
    /// other 5xx). Not for any other 429: the hourly, daily and monthly
    /// sending limits last until `retry_after`, and the same send is
    /// refused again until then. Not for `unavailable` either, which
    /// means the service at the base URL does not offer the call at all.
    /// A send is repeated with the same `Idempotency-Key`, so that it
    /// cannot go out twice.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Api {
                status: 429, code, ..
            } => code == "rate_limited",
            Error::Api { status, code, .. } => *status >= 500 && *status != 501 && code != "unavailable",
            Error::Transport(_) => true,
            _ => false,
        }
    }

    /// True when the service answered and the answer says the request
    /// changed nothing: every refusal below 500, a `503 try_later` and
    /// `unavailable`. After any other failure, a broken connection or a
    /// 500, nobody knows whether a send went out, and it is repeated
    /// with the same `Idempotency-Key` to find out.
    pub fn changed_nothing(&self) -> bool {
        match self {
            Error::Api { status, code, .. } => {
                *status < 500
                    || (*status == 503 && (code == "try_later" || code == "unavailable"))
                    || (*status == 501 && code == "unavailable")
            }
            Error::Config(_) => true,
            Error::Transport(_) | Error::Decode(_) => false,
        }
    }

    /// True for the answer kept under an `Idempotency-Key` from an
    /// earlier request with it, given again instead of doing anything.
    pub fn is_replayed(&self) -> bool {
        matches!(self, Error::Api { replayed: true, .. })
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
