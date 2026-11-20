use crate::error::{Details, Reason};
use crate::{Capabilities, DecisionRequest, DecisionResponse, Error, ErrorCode, OpenAi, SystemOne};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
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

/// Per-client settings. Debug output redacts the key, header values and
/// option values.
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
    /// Extra headers (gateways, tenants, tracing). Sent after the adapter's
    /// own; one with the same name (any case) replaces the adapter's.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// Free-form settings for custom providers. Built-ins ignore them.
    #[serde(default)]
    pub options: Map<String, Value>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let header_names: Vec<&str> = self.headers.iter().map(|(k, _)| k.as_str()).collect();
        let option_names: Vec<&str> = self.options.keys().map(String::as_str).collect();
        f.debug_struct("Config")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "<redacted>"),
            )
            // Query strings can carry tokens.
            .field(
                "base_url",
                &self.base_url.as_deref().map(|u| match u.split_once('?') {
                    Some((path, _)) => format!("{path}?<redacted>"),
                    None => u.to_owned(),
                }),
            )
            .field("account_id", &self.account_id)
            .field("headers", &header_names)
            .field("options", &option_names)
            .finish()
    }
}

/// The whole extension surface: pure functions. No I/O, no clock, no globals.
pub trait Provider {
    /// What this provider (and model) can take. Checked before `build` when
    /// going through a [`Registry`] or the JSON layer.
    fn capabilities(&self, model: &str) -> Capabilities;
    /// Canonical request → HTTP request. Validates first.
    fn build(&self, model: &str, req: &DecisionRequest, cfg: &Config)
        -> Result<HttpRequest, Error>;
    /// HTTP response → canonical response, or a classified error.
    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error>;
}

/// Prefixes of the built-in adapters. A [`Registry`] refuses to register
/// these names unless asked to [`replace`](Registry::replace).
pub const BUILTIN: [&str; 5] = ["jev", "clef", "gemini", "openai", "systemone"];

fn builtin(name: &str) -> Option<Box<dyn Provider + Send + Sync>> {
    match name {
        "openai" => Some(Box::new(OpenAi)),
        _ => SystemOne::preset(name).map(|p| Box::new(p) as Box<dyn Provider + Send + Sync>),
    }
}

/// `"provider/model"` → `("provider", "model")`.
fn split(model_ref: &str) -> Result<(&str, &str), Error> {
    model_ref
        .split_once('/')
        .filter(|(p, m)| !p.is_empty() && !m.is_empty())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidRequest,
                "model must look like `provider/model`",
            )
        })
}

fn unknown(provider: &str) -> Error {
    let mut d = Details::new(Reason::UnknownProvider);
    d.provider = Some(provider.to_owned());
    Error::detailed(
        ErrorCode::Unsupported,
        format!("unknown provider `{provider}`"),
        d,
    )
}

/// Splits `"provider/model"` and returns the built-in adapter plus the model
/// name. Shortcut for [`Registry::builtin`] when no custom provider is used.
pub fn resolve(model_ref: &str) -> Result<(Box<dyn Provider>, &str), Error> {
    let (provider, model) = split(model_ref)?;
    let adapter = builtin(provider).ok_or_else(|| unknown(provider))?;
    Ok((adapter, model))
}

/// Providers by prefix. A value, not a global: each client owns one.
///
/// ```
/// # use letsdecide::*;
/// let mut providers = Registry::builtin();
/// // A custom provider is any `Provider`; here a System One preset stands in.
/// providers.register("acme", SystemOne::preset("systemone").unwrap()).unwrap();
/// assert!(providers.register("openai", OpenAi).is_err()); // taken
/// ```
pub struct Registry {
    providers: BTreeMap<String, Box<dyn Provider + Send + Sync>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Registry {
    /// No providers.
    pub fn empty() -> Self {
        Self {
            providers: BTreeMap::new(),
        }
    }

    /// The built-in adapters (see [`BUILTIN`]).
    pub fn builtin() -> Self {
        let providers = BUILTIN
            .iter()
            .map(|n| ((*n).to_owned(), builtin(n).expect("built-in exists")))
            .collect();
        Self { providers }
    }

    /// Adds a provider. Fails with `InvalidRequest` (`name_taken`) if the
    /// name is in use, so a custom provider never shadows another by accident.
    pub fn register(
        &mut self,
        name: &str,
        provider: impl Provider + Send + Sync + 'static,
    ) -> Result<(), Error> {
        check_name(name)?;
        if self.providers.contains_key(name) {
            let mut d = Details::new(Reason::NameTaken);
            d.provider = Some(name.to_owned());
            return Err(Error::detailed(
                ErrorCode::InvalidRequest,
                format!("provider `{name}` is already registered; use `replace` to override it"),
                d,
            ));
        }
        self.providers.insert(name.to_owned(), Box::new(provider));
        Ok(())
    }

    /// Adds or overrides a provider, built-ins included. Returns the old one.
    pub fn replace(
        &mut self,
        name: &str,
        provider: impl Provider + Send + Sync + 'static,
    ) -> Result<Option<Box<dyn Provider + Send + Sync>>, Error> {
        check_name(name)?;
        Ok(self.providers.insert(name.to_owned(), Box::new(provider)))
    }

    pub fn remove(&mut self, name: &str) -> Option<Box<dyn Provider + Send + Sync>> {
        self.providers.remove(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.providers.keys().map(String::as_str)
    }

    /// `"provider/model"` → the provider and the model name.
    pub fn resolve<'m>(&self, model_ref: &'m str) -> Result<(&dyn Provider, &'m str), Error> {
        let (name, model) = split(model_ref)?;
        let p = self.providers.get(name).ok_or_else(|| unknown(name))?;
        Ok((p.as_ref(), model))
    }

    /// Capabilities of `"provider/model"`.
    pub fn capabilities(&self, model_ref: &str) -> Result<Capabilities, Error> {
        let (p, model) = self.resolve(model_ref)?;
        Ok(p.capabilities(model))
    }

    /// Checks the request against the provider's capabilities, then builds.
    /// Errors name the provider in their details.
    pub fn build(
        &self,
        model_ref: &str,
        req: &DecisionRequest,
        cfg: &Config,
    ) -> Result<HttpRequest, Error> {
        let (name, _) = split(model_ref)?;
        let (p, model) = self.resolve(model_ref)?;
        p.capabilities(model)
            .check(req)
            .and_then(|()| p.build(model, req, cfg))
            .map_err(|e| e.with_provider(name))
    }

    /// Parses, then [checks](DecisionResponse::check) the response, so every
    /// provider gives the same guarantees.
    pub fn parse(
        &self,
        model_ref: &str,
        req: &DecisionRequest,
        res: &HttpResponse,
    ) -> Result<DecisionResponse, Error> {
        let (name, _) = split(model_ref)?;
        let (p, _) = self.resolve(model_ref)?;
        p.parse(req, res)
            .and_then(|r| r.check(req).map(|()| r))
            .map_err(|e| e.with_provider(name))
    }
}

/// `[a-z0-9_-]{1,64}`: no `/`, so `name/model` always splits.
fn check_name(name: &str) -> Result<(), Error> {
    let ok = (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(Error::detailed(
            ErrorCode::InvalidRequest,
            "provider names must match `[a-z0-9_-]{1,64}`",
            Details::new(Reason::InvalidConfig).field("name"),
        ))
    }
}

/// Classifies a non-2xx reply. Never echoes the body. Public so custom
/// providers classify errors the same way as the built-ins.
pub fn http_error(res: &HttpResponse) -> Error {
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

/// Shared by adapters: the JSON body plus `provider_extras`. An extra key the
/// adapter already sets is an error, never a silent overwrite.
pub(crate) fn encode<T: Serialize>(body: &T, req: &DecisionRequest) -> Result<Vec<u8>, Error> {
    let invalid = |m: String| Error::new(ErrorCode::InvalidRequest, m);
    if req.provider_extras.is_empty() {
        return serde_json::to_vec(body).map_err(|e| invalid(e.to_string()));
    }
    let mut v = serde_json::to_value(body).map_err(|e| invalid(e.to_string()))?;
    let obj = v.as_object_mut().expect("bodies are objects");
    for (k, x) in &req.provider_extras {
        if obj.contains_key(k) {
            return Err(invalid(format!(
                "provider_extras: `{k}` is set by the adapter"
            )));
        }
        obj.insert(k.clone(), x.clone());
    }
    serde_json::to_vec(&v).map_err(|e| invalid(e.to_string()))
}

/// Shared by adapters: JSON POST, key as `Bearer` or raw in a custom header.
/// Adapters call [`apply_headers`] last.
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

/// Appends `cfg.headers`, replacing same-named ones. Public for custom
/// providers. Rejects names and values that could split a header
/// (`InvalidRequest`, `invalid_config`); the error names the header, never
/// its value.
pub fn apply_headers(http: &mut HttpRequest, cfg: &Config) -> Result<(), Error> {
    for (name, value) in &cfg.headers {
        let name_ok = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
        if !name_ok {
            return Err(Error::detailed(
                ErrorCode::InvalidRequest,
                "a header name in `headers` is not a valid HTTP token",
                Details::new(Reason::InvalidConfig).field("headers"),
            ));
        }
        if value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
            return Err(Error::detailed(
                ErrorCode::InvalidRequest,
                format!("header `{name}` has a line break in its value"),
                Details::new(Reason::InvalidConfig).field("headers"),
            ));
        }
        http.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        http.headers.push((name.clone(), value.clone()));
    }
    Ok(())
}
