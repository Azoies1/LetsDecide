use crate::{DecisionRequest, DecisionResponse, Error, ErrorCode, OpenAi, SystemOne};
use serde::{Deserialize, Serialize};
use std::fmt;

/// An HTTP request for the host to send. Nothing here does I/O.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The host's reply, passed back for parsing.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    /// Header names compared case-insensitively.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Per-client settings. Debug output redacts the key.
#[derive(Clone, Default, Deserialize)]
pub struct Config {
    /// Static key. Required for `gemini/*` (sent as `x-goog-api-key`).
    pub api_key: Option<String>,
    /// OAuth access token, sent as `Authorization: Bearer`. Used by `clef/*`
    /// (and the other Bearer providers) in place of `api_key`; it wins when
    /// both are set. The core never fetches or refreshes tokens: the host does
    /// that and passes the current token in.
    pub access_token: Option<String>,
    /// Required for `systemone/*`; overrides the preset default otherwise.
    pub base_url: Option<String>,
    /// Required for `clef/*`.
    pub account_id: Option<String>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "<redacted>"),
            )
            .field("base_url", &self.base_url)
            .field("account_id", &self.account_id)
            .finish()
    }
}

/// The whole extension surface: two pure functions. No I/O, no clock, no globals.
pub trait Provider {
    /// Canonical request → HTTP request. Validates first.
    fn build(&self, model: &str, req: &DecisionRequest, cfg: &Config)
        -> Result<HttpRequest, Error>;
    /// HTTP response → canonical response, or a classified error.
    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error>;
}

/// Splits `"provider/model"` and returns the adapter plus the model name.
pub fn resolve(model_ref: &str) -> Result<(Box<dyn Provider>, &str), Error> {
    let (provider, model) = model_ref
        .split_once('/')
        .filter(|(_, m)| !m.is_empty())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidRequest,
                "model must look like `provider/model`",
            )
        })?;
    let adapter: Box<dyn Provider> = match provider {
        "openai" => Box::new(OpenAi),
        _ => match SystemOne::preset(provider) {
            Some(p) => Box::new(p),
            None => {
                let m = format!("unknown provider `{provider}`");
                return Err(Error::new(ErrorCode::Unsupported, m));
            }
        },
    };
    Ok((adapter, model))
}

/// Shared by adapters: classify a non-2xx reply. Never echoes the body.
pub(crate) fn http_error(res: &HttpResponse) -> Error {
    let code = match res.status {
        401 | 403 => ErrorCode::Auth,
        429 => ErrorCode::RateLimited,
        500..=599 => ErrorCode::ProviderUnavailable,
        _ => ErrorCode::InvalidRequest,
    };
    let retry_after_ms = res
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, v)| v.trim().parse::<u64>().ok())
        .map(|s| s.saturating_mul(1000));
    let message = format!("HTTP {}", res.status);
    Error {
        status: Some(res.status),
        retry_after_ms,
        ..Error::new(code, message)
    }
}

/// Shared by adapters: JSON POST, key as `Bearer` or raw in a custom header.
pub(crate) fn post(
    url: String,
    key_header: &str,
    bearer: bool,
    cfg: &Config,
    body: Vec<u8>,
) -> HttpRequest {
    let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
    let credential = if bearer {
        cfg.access_token
            .as_ref()
            .or(cfg.api_key.as_ref())
            .map(|t| format!("Bearer {t}"))
    } else {
        cfg.api_key.clone()
    };
    if let Some(value) = credential {
        headers.push((key_header.to_owned(), value));
    }
    HttpRequest {
        method: "POST",
        url,
        headers,
        body,
    }
}
