#![doc = include_str!("../README.md")]

mod cost;
mod error;
mod gemini;
pub mod json;
mod model;
mod openai;
mod provider;
mod systemone;

pub use cost::estimate_cost_usd;
pub use error::{Error, ErrorCode};
pub use model::*;
pub use openai::OpenAi;
pub use provider::{resolve, Config, HttpRequest, HttpResponse, Provider};
pub use systemone::SystemOne;
