use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable code apps can branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidRequest,
    Unsupported,
    Auth,
    RateLimited,
    ProviderUnavailable,
    /// 200 reply that doesn't parse or fails [`DecisionResponse::check`](crate::DecisionResponse::check):
    /// likely an API change, or a bug in a custom provider.
    BadResponse,
    /// A custom provider or engine (user code) failed with its own error.
    ProviderError,
}

/// Why a request failed, in a form apps can branch on. Stable wire names
/// (`snake_case`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Reason {
    // Request vs provider capabilities (`Unsupported`).
    TooManyQuestions,
    TooManyOptions,
    TooManyLevels,
    TooManyMedia,
    MediaNotAccepted,
    QuestionKind,
    UnknownProvider,
    /// Local runtime can't give log-probs.
    NoLogprobs,
    /// A scoring label spans more than one token.
    MultiTokenLabel,
    // Request or config problems (`InvalidRequest`).
    MissingConfig,
    InvalidConfig,
    NameTaken,
    // Response problems (`BadResponse`).
    MissingAnswer,
    UnexpectedAnswer,
    WrongAnswerKind,
    UnknownId,
    InvalidNumber,
}

/// Structured context for an [`Error`].
///
/// Every field is safe to log by construction: names, ids, counts, MIME
/// types and config *field names*. Never keys, header values, input, media,
/// instructions, descriptions, bodies or config values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Details {
    pub reason: Reason,
    /// Provider prefix, e.g. `clef`. Filled in by the JSON layer and the
    /// registry when the adapter doesn't set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Question id (caller-chosen, `[a-z0-9_]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// Option or level id (caller-chosen, `[a-z0-9_]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Config field name, never its value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl Details {
    pub fn new(reason: Reason) -> Self {
        Self {
            reason,
            provider: None,
            question: None,
            id: None,
            limit: None,
            actual: None,
            mime_type: None,
            field: None,
        }
    }

    pub fn question(mut self, id: &str) -> Self {
        self.question = Some(id.to_owned());
        self
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = Some(id.to_owned());
        self
    }

    pub fn limit(mut self, limit: usize, actual: usize) -> Self {
        self.limit = Some(limit as u64);
        self.actual = Some(actual as u64);
        self
    }

    pub fn mime_type(mut self, mime: &str) -> Self {
        self.mime_type = Some(mime.to_owned());
        self
    }

    pub fn field(mut self, name: &str) -> Self {
        self.field = Some(name.to_owned());
        self
    }
}

/// One error type for every provider. Never carries keys or request bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub status: Option<u16>,
    pub retry_after_ms: Option<u64>,
    /// Boxed: keeps `Result<_, Error>` small on the happy path.
    pub details: Option<Box<Details>>,
}

impl ErrorCode {
    /// Stable wire name used by the bindings.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "InvalidRequest",
            Self::Unsupported => "Unsupported",
            Self::Auth => "Auth",
            Self::RateLimited => "RateLimited",
            Self::ProviderUnavailable => "ProviderUnavailable",
            Self::BadResponse => "BadResponse",
            Self::ProviderError => "ProviderError",
        }
    }

    /// Inverse of [`as_str`](Self::as_str).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "InvalidRequest" => Self::InvalidRequest,
            "Unsupported" => Self::Unsupported,
            "Auth" => Self::Auth,
            "RateLimited" => Self::RateLimited,
            "ProviderUnavailable" => Self::ProviderUnavailable,
            "BadResponse" => Self::BadResponse,
            "ProviderError" => Self::ProviderError,
            _ => return None,
        })
    }
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            retry_after_ms: None,
            details: None,
        }
    }

    /// Shorthand for an error with [`Details`].
    pub fn detailed(code: ErrorCode, message: impl Into<String>, details: Details) -> Self {
        Self {
            details: Some(Box::new(details)),
            ..Self::new(code, message)
        }
    }

    /// Records which provider failed, unless the adapter already did.
    pub fn with_provider(mut self, provider: &str) -> Self {
        if let Some(d) = &mut self.details {
            d.provider.get_or_insert_with(|| provider.to_owned());
        }
        self
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

/// serde's messages can quote the offending value (e.g. `invalid type:
/// string "…"`), which may be user input or a response body. Keep only
/// messages that name our own fields, plus the position.
pub(crate) fn safe_serde_message(what: &str, e: &serde_json::Error) -> String {
    let msg = e.to_string();
    let ours = ["missing field `", "duplicate field `"]
        .iter()
        .any(|p| msg.starts_with(p));
    let kind = match e.classify() {
        serde_json::error::Category::Io => "unreadable",
        serde_json::error::Category::Syntax => "not valid JSON",
        serde_json::error::Category::Data => "the wrong shape",
        serde_json::error::Category::Eof => "truncated",
    };
    if ours {
        // `missing field `x` at line 1 column 2`: field names are ours.
        format!("{what}: {msg}")
    } else {
        format!(
            "{what} is {kind} (line {}, column {})",
            e.line(),
            e.column()
        )
    }
}
