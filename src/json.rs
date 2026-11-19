//! JSON in, JSON out, for bindings. Errors are returned as data
//! (`{"error": ...}`) so they cross any FFI boundary without exceptions;
//! the host wrapper rethrows them.

use crate::{resolve, Config, DecisionRequest, Error, ErrorCode, HttpResponse};
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
}

impl From<Error> for ErrorOut {
    fn from(e: Error) -> Self {
        Self {
            retryable: e.retryable(),
            code: e.code.as_str().to_owned(),
            message: e.message,
            status: e.status,
            retry_after_ms: e.retry_after_ms,
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

fn invalid(e: serde_json::Error) -> Error {
    Error::new(ErrorCode::InvalidRequest, e.to_string())
}

/// `model`: `"provider/model"`. `request`, `config`: JSON. Returns an HTTP request.
pub fn build(model: &str, request: &str, config: &str) -> String {
    run(|| {
        let req: DecisionRequest = serde_json::from_str(request).map_err(invalid)?;
        let cfg: Config = serde_json::from_str(config).map_err(invalid)?;
        let (provider, name) = resolve(model)?;
        let http = provider.build(name, &req, &cfg)?;
        Ok(HttpOut {
            method: http.method,
            url: http.url,
            headers: http.headers,
            body: String::from_utf8(http.body).expect("JSON is UTF-8"),
        })
    })
}

/// `response`: `{status, headers: [[k, v]], body}`. Returns the canonical response.
pub fn parse(model: &str, request: &str, response: &str) -> String {
    run(|| {
        let req: DecisionRequest = serde_json::from_str(request).map_err(invalid)?;
        let res: HttpIn = serde_json::from_str(response).map_err(invalid)?;
        let (provider, _) = resolve(model)?;
        provider.parse(
            &req,
            &HttpResponse {
                status: res.status,
                headers: res.headers,
                body: res.body.into_bytes(),
            },
        )
    })
}
