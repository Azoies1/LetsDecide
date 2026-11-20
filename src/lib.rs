#![doc = include_str!("../README.md")]

mod capabilities;
mod cost;
mod dialect;
mod error;
mod gemini;
pub mod json;
mod model;
mod openai;
mod provider;
mod systemone;

pub use capabilities::{Capabilities, Locality, ProbabilitySource};
pub use cost::estimate_cost_usd;
pub use dialect::{Auth, AuthScheme, Dialect, Family, InputField, MediaEncoding};
pub use error::{Details, Error, ErrorCode, Reason};
pub use model::*;
pub use provider::{
    apply_headers, http_error, resolve, Config, HttpRequest, HttpResponse, Provider, Registry,
    BUILTIN,
};
