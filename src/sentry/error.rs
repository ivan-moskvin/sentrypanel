//! What can go wrong between us and Sentry, sorted by whom it concerns: the
//! whole panel, or the one metric whose request was refused.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The token is wrong, expired or lacks a scope.
    Auth(u16),
    /// Sentry could not be reached at all.
    Offline(String),
    RateLimited,
    Server(u16),
    /// Sentry refused this request — usually a field or a query it does not
    /// understand. Other requests may well work.
    Rejected(String),
    Malformed(String),
}

impl FetchError {
    pub fn from_status(status: u16, detail: String) -> FetchError {
        match status {
            401 | 403 => FetchError::Auth(status),
            429 => FetchError::RateLimited,
            500.. => FetchError::Server(status),
            _ => FetchError::Rejected(format!("HTTP {status}: {detail}")),
        }
    }

    /// Whether every other request would fail the same way, so the panel as a
    /// whole has to say so instead of showing old numbers as if they were new.
    pub fn is_global(&self) -> bool {
        !matches!(self, FetchError::Rejected(_) | FetchError::Malformed(_))
    }

    /// The word in the corner of the panel.
    pub fn short(&self) -> String {
        match self {
            FetchError::Auth(_) => "AUTH".into(),
            FetchError::Offline(_) => "OFFLINE".into(),
            FetchError::RateLimited => "LIMIT".into(),
            FetchError::Server(status) => format!("HTTP {status}"),
            FetchError::Rejected(_) | FetchError::Malformed(_) => "ERROR".into(),
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Auth(status) => write!(
                f,
                "Sentry refused the token (HTTP {status}): check it and its org:read scope"
            ),
            FetchError::Offline(what) => write!(f, "Sentry is unreachable: {what}"),
            FetchError::RateLimited => write!(f, "Sentry rate limit hit (HTTP 429)"),
            FetchError::Server(status) => write!(f, "Sentry failed (HTTP {status})"),
            FetchError::Rejected(what) => write!(f, "Sentry refused the request: {what}"),
            FetchError::Malformed(what) => write!(f, "{what}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_a_status_into_what_it_means() {
        assert_eq!(
            FetchError::from_status(401, String::new()),
            FetchError::Auth(401)
        );
        assert_eq!(
            FetchError::from_status(403, String::new()),
            FetchError::Auth(403)
        );
        assert_eq!(
            FetchError::from_status(429, String::new()),
            FetchError::RateLimited
        );
        assert_eq!(
            FetchError::from_status(502, String::new()),
            FetchError::Server(502)
        );
        assert_eq!(
            FetchError::from_status(400, "Invalid query".into()),
            FetchError::Rejected("HTTP 400: Invalid query".into())
        );
    }

    #[test]
    fn concerns_the_panel_only_when_every_request_would_fail() {
        assert!(FetchError::Auth(401).is_global());
        assert!(FetchError::Offline("timeout".into()).is_global());
        assert!(FetchError::RateLimited.is_global());
        assert!(FetchError::Server(503).is_global());
        assert!(!FetchError::Rejected("bad field".into()).is_global());
        assert!(!FetchError::Malformed("not json".into()).is_global());
    }

    #[test]
    fn names_the_trouble_in_a_word_the_panel_can_draw() {
        for error in [
            FetchError::Auth(401),
            FetchError::Offline("x".into()),
            FetchError::RateLimited,
            FetchError::Server(503),
            FetchError::Rejected("x".into()),
        ] {
            let word = error.short();
            assert!(divoomkit::font::covers(&word), "{word}");
            assert!(word.chars().count() <= 8, "{word}");
            assert!(!error.to_string().is_empty());
        }
    }
}
