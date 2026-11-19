//! WASM wrapper over `letsdecide::json`.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn build(model: &str, request: &str, config: &str) -> String {
    letsdecide::json::build(model, request, config)
}

#[wasm_bindgen]
pub fn parse(model: &str, request: &str, response: &str) -> String {
    letsdecide::json::parse(model, request, response)
}
