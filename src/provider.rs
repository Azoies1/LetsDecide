use crate::{DecisionRequest, DecisionResponse, Error, ErrorCode, SystemOne};
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
    pub api_key: Option<String>,
    /// Required for `systemone/*`; overrides the preset default otherwise.
    pub base_url: Option<String>,
    /// Required for `clef/*`.
    pub account_id: Option<String>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
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
    match SystemOne::preset(provider) {
        Some(p) => Ok((Box::new(p), model)),
        None => Err(Error::new(
            ErrorCode::Unsupported,
            format!("unknown provider `{provider}`"),
        )),
    }
}
