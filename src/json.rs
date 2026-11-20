//! JSON in, JSON out, for bindings. Errors are returned as data
//! (`{"error": ...}`) so they cross any FFI boundary without exceptions;
//! the host wrapper rethrows them.

use crate::error::safe_serde_message;
use crate::{
    estimate_cost_usd, http_error, Capabilities, Config, DecisionRequest, DecisionResponse,
    Details, Dialect, Error, ErrorCode, HttpResponse, Provider, Registry,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Out<T> {
    Ok(T),
    Error(ErrorOut),
}

#[derive(Serialize)]
struct ErrorOut {
    code: String,
    message: String,
    status: Option<u16>,
    retry_after_ms: Option<u64>,
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Box<Details>>,
}

impl From<Error> for ErrorOut {
    fn from(e: Error) -> Self {
        Self {
            retryable: e.retryable(),
            code: e.code.as_str().to_owned(),
            message: e.message,
            status: e.status,
            retry_after_ms: e.retry_after_ms,
            details: e.details,
        }
    }
}

/// The body is always JSON, so it travels as text.
#[derive(Serialize)]
struct HttpOut {
    method: &'static str,
    url: String,
    headers: Vec<(String, String)>,
    body: String,
}

// Serialised directly (not via `Value`) so f32 probabilities keep their short form.
#[derive(Serialize)]
struct Parsed {
    #[serde(flatten)]
    response: DecisionResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    estimated_cost_usd: Option<f64>,
}

#[derive(Deserialize)]
struct HttpIn {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

fn run<T: Serialize>(f: impl FnOnce() -> Result<T, Error>) -> String {
    let out = match f() {
        Ok(v) => Out::Ok(v),
        Err(e) => Out::Error(e.into()),
    };
    serde_json::to_string(&out).expect("output serialises")
}

/// Never echoes the offending value: it may be user input.
fn invalid(what: &'static str) -> impl Fn(serde_json::Error) -> Error {
    move |e| Error::new(ErrorCode::InvalidRequest, safe_serde_message(what, &e))
}

fn http_in(response: &str) -> Result<HttpResponse, Error> {
    let res: HttpIn = serde_json::from_str(response).map_err(invalid("response"))?;
    Ok(HttpResponse {
        status: res.status,
        headers: res.headers,
        body: res.body.into_bytes(),
    })
}

/// `model`: `"provider/model"`. `request`, `config`: JSON. Returns an HTTP request.
pub fn build(model: &str, request: &str, config: &str) -> String {
    run(|| {
        let req: DecisionRequest = serde_json::from_str(request).map_err(invalid("request"))?;
        let cfg: Config = serde_json::from_str(config).map_err(invalid("config"))?;
        let http = Registry::builtin().build(model, &req, &cfg)?;
        Ok(HttpOut {
            method: http.method,
            url: http.url,
            headers: http.headers,
            body: String::from_utf8(http.body).expect("JSON is UTF-8"),
        })
    })
}

/// `response`: `{status, headers: [[k, v]], body}`. Returns the canonical
/// response, already [checked](DecisionResponse::check), plus
/// `estimated_cost_usd` when the model's price is known.
pub fn parse(model: &str, request: &str, response: &str) -> String {
    run(|| {
        let req: DecisionRequest = serde_json::from_str(request).map_err(invalid("request"))?;
        let out = Registry::builtin().parse(model, &req, &http_in(response)?)?;
        Ok(Parsed {
            estimated_cost_usd: out.input_tokens.and_then(|t| estimate_cost_usd(model, t)),
            response: out,
        })
    })
}

/// A built-in dialect (`jev`, `clef`, `gemini`, `openai`, `systemone`) as
/// JSON, to read or copy as a starting point.
pub fn dialect(name: &str) -> String {
    run(|| {
        Dialect::builtin_json(name).ok_or_else(|| {
            let mut d = Details::new(crate::Reason::UnknownProvider);
            d.provider = Some(name.to_owned());
            Error::detailed(
                ErrorCode::Unsupported,
                format!("no built-in dialect `{name}`"),
                d,
            )
        })
    })
}

/// Validates a dialect and expands `extends`. Returns the full dialect, which
/// `build_dialect` / `parse_dialect` take. Bindings call this once, when the
/// dialect is registered.
pub fn resolve_dialect(dialect: &str) -> String {
    run(|| Dialect::from_json(dialect))
}

fn dialect_in(dialect: &str) -> Result<Dialect, Error> {
    Dialect::from_json(dialect)
}

/// As [`build`], with a dialect instead of a built-in. `model` is the model
/// name alone (no `provider/` prefix). Capabilities are checked first.
pub fn build_dialect(dialect: &str, model: &str, request: &str, config: &str) -> String {
    run(|| {
        let d = dialect_in(dialect)?;
        let req: DecisionRequest = serde_json::from_str(request).map_err(invalid("request"))?;
        let cfg: Config = serde_json::from_str(config).map_err(invalid("config"))?;
        d.capabilities.check(&req)?;
        let http = d.build(model, &req, &cfg)?;
        Ok(HttpOut {
            method: http.method,
            url: http.url,
            headers: http.headers,
            body: String::from_utf8(http.body).expect("JSON is UTF-8"),
        })
    })
}

/// As [`parse`], with a dialect. The response is [checked](DecisionResponse::check).
pub fn parse_dialect(dialect: &str, request: &str, response: &str) -> String {
    run(|| {
        let d = dialect_in(dialect)?;
        let req: DecisionRequest = serde_json::from_str(request).map_err(invalid("request"))?;
        let out = d.parse(&req, &http_in(response)?)?;
        out.check(&req)?;
        Ok(out)
    })
}

/// Names of the built-in providers. Bindings refuse these as custom names.
pub fn builtins() -> String {
    run(|| Ok(crate::BUILTIN))
}

/// Capabilities of a built-in `"provider/model"`.
pub fn capabilities(model: &str) -> String {
    run(|| Registry::builtin().capabilities(model))
}

/// For custom providers: validates `request` and, unless `capabilities` is
/// `null`, checks it against them. Returns `null`.
pub fn validate(request: &str, capabilities: &str) -> String {
    run(|| {
        let req: DecisionRequest = serde_json::from_str(request).map_err(invalid("request"))?;
        let caps: Option<Capabilities> =
            serde_json::from_str(capabilities).map_err(invalid("capabilities"))?;
        match caps {
            Some(c) => c.check(&req),
            None => req.validate(),
        }
    })
}

/// For custom providers: [checks](DecisionResponse::check) a canonical
/// `response` against `request`. Returns the response, with defaults filled.
pub fn check(request: &str, response: &str) -> String {
    run(|| {
        let req: DecisionRequest = serde_json::from_str(request).map_err(invalid("request"))?;
        let res: DecisionResponse = serde_json::from_str(response).map_err(|e| {
            Error::new(
                ErrorCode::BadResponse,
                safe_serde_message("decision response", &e),
            )
        })?;
        res.check(&req).map(|()| res)
    })
}

/// For custom providers: classifies a non-2xx HTTP reply like the built-ins
/// do. Returns the error object as `ok`, for the host to throw.
pub fn classify_http(response: &str) -> String {
    run(|| Ok(ErrorOut::from(http_error(&http_in(response)?))))
}
