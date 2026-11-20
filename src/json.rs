//! JSON in, JSON out, for bindings. Errors are returned as data
//! (`{"error": ...}`) so they cross any FFI boundary without exceptions;
//! the host wrapper rethrows them.

use crate::error::safe_serde_message;
use crate::{
    estimate_cost_usd, http_error, Capabilities, Config, DecisionRequest, DecisionResponse,
    Details, Error, ErrorCode, HttpResponse, Member, Outcome, Reason, Registry, Rules,
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

/// A member's outcome as the host reports it: exactly one of `response`,
/// `error` (a thrown `LetsDecideError`'s fields) or `skipped: true`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberIn {
    model: String,
    #[serde(default = "one")]
    weight: f32,
    questions: Option<Vec<String>>,
    response: Option<DecisionResponse>,
    error: Option<MemberError>,
    #[serde(default)]
    skipped: bool,
}

fn one() -> f32 {
    1.0
}

/// The host's message is dropped: it may come from user code.
#[derive(Deserialize)]
struct MemberError {
    code: String,
    status: Option<u16>,
    retry_after_ms: Option<u64>,
    details: Option<Box<Details>>,
}

/// Panels: merges member outcomes (`[MemberIn]`) under `rules`
/// (`{quorum?, merge?}`). Returns the merged response plus `agreement`
/// and `disagreed`.
pub fn merge(request: &str, members: &str, rules: &str) -> String {
    run(|| {
        let req: DecisionRequest = serde_json::from_str(request).map_err(invalid("request"))?;
        let rules: Rules = serde_json::from_str(rules).map_err(invalid("rules"))?;
        let raw: Vec<MemberIn> = serde_json::from_str(members).map_err(invalid("members"))?;
        let members = raw
            .into_iter()
            .map(|m| {
                let outcome = match (m.response, m.error, m.skipped) {
                    (Some(r), None, false) => Outcome::Answered(r),
                    (None, Some(e), false) => Outcome::Failed(Error {
                        status: e.status,
                        retry_after_ms: e.retry_after_ms,
                        details: e.details,
                        ..Error::new(
                            ErrorCode::from_name(&e.code).unwrap_or(ErrorCode::ProviderError),
                            "member failed",
                        )
                    }),
                    (None, None, true) => Outcome::Skipped,
                    _ => {
                        return Err(Error::detailed(
                            ErrorCode::InvalidRequest,
                            "each member needs exactly one of `response`, `error`, `skipped`",
                            Details::new(Reason::InvalidConfig).field("members"),
                        ))
                    }
                };
                Ok(Member {
                    model: m.model,
                    questions: m.questions,
                    weight: m.weight,
                    outcome,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        crate::merge(&req, &members, &rules)
    })
}
