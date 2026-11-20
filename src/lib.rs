#![doc = include_str!("../README.md")]

mod capabilities;
mod cost;
mod error;
mod gemini;
pub mod json;
mod model;
mod openai;
mod provider;
mod systemone;

pub use capabilities::{Capabilities, Locality, ProbabilitySource};
pub use cost::estimate_cost_usd;
pub use error::{Details, Error, ErrorCode, Reason};
pub use model::*;
pub use openai::OpenAi;
pub use provider::{
    apply_headers, http_error, resolve, Config, HttpRequest, HttpResponse, Provider, Registry,
    BUILTIN,
};
pub use systemone::SystemOne;
