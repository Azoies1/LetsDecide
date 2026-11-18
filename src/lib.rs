#![doc = include_str!("../README.md")]

mod error;
mod model;
mod provider;
mod systemone;

pub use error::{Error, ErrorCode};
pub use model::*;
pub use provider::{resolve, Config, HttpRequest, HttpResponse, Provider};
pub use systemone::SystemOne;
