#![doc = include_str!("../README.md")]

mod error;
pub mod json;
mod model;
mod openai;
mod provider;
mod systemone;

pub use error::{Error, ErrorCode};
pub use model::*;
pub use openai::OpenAi;
pub use provider::{resolve, Config, HttpRequest, HttpResponse, Provider};
pub use systemone::SystemOne;
