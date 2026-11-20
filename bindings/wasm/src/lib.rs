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

#[wasm_bindgen]
pub fn builtins() -> String {
    letsdecide::json::builtins()
}

#[wasm_bindgen]
pub fn capabilities(model: &str) -> String {
    letsdecide::json::capabilities(model)
}

#[wasm_bindgen]
pub fn validate(request: &str, capabilities: &str) -> String {
    letsdecide::json::validate(request, capabilities)
}

#[wasm_bindgen]
pub fn check(request: &str, response: &str) -> String {
    letsdecide::json::check(request, response)
}

#[wasm_bindgen]
pub fn classify_http(response: &str) -> String {
    letsdecide::json::classify_http(response)
}
