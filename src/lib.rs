#![doc = include_str!("../README.md")]

mod capabilities;
mod error;
pub mod json;
mod model;
mod openai;
mod provider;
mod systemone;

pub use capabilities::{Capabilities, Locality, ProbabilitySource};
pub use error::{Details, Error, ErrorCode, Reason};
pub use model::*;
pub use openai::OpenAi;
pub use provider::{
    apply_headers, http_error, resolve, Config, HttpRequest, HttpResponse, Provider, Registry,
    BUILTIN,
};
pub use systemone::SystemOne;
