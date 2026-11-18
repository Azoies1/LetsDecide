use std::fmt;

/// Stable code apps can branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidRequest,
    Unsupported,
    Auth,
    RateLimited,
    ProviderUnavailable,
    /// 200 reply that doesn't parse: likely an API change.
    BadResponse,
}

/// One error type for every provider. Never carries keys or request bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub status: Option<u16>,
    pub retry_after_ms: Option<u64>,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            retry_after_ms: None,
        }
    }

    /// Whether a retry may succeed.
    pub fn retryable(&self) -> bool {
        matches!(
            self.code,
            ErrorCode::RateLimited | ErrorCode::ProviderUnavailable
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}
