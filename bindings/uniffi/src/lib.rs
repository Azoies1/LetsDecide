//! Swift and Kotlin wrapper over `letsdecide::json` (UniFFI). JSON strings in
//! and out, as in the other bindings; the host wrapper sends the request.

uniffi::setup_scaffolding!();

/// `model`: `"provider/model"`. Returns `{"ok": http}` or `{"error": ...}`.
#[uniffi::export]
pub fn build(model: String, request: String, config: String) -> String {
    letsdecide::json::build(&model, &request, &config)
}

/// Returns `{"ok": response}` or `{"error": ...}`.
#[uniffi::export]
pub fn parse(model: String, request: String, response: String) -> String {
    letsdecide::json::parse(&model, &request, &response)
}

/// Capabilities of a built-in `"provider/model"`.
#[uniffi::export]
pub fn capabilities(model: String) -> String {
    letsdecide::json::capabilities(&model)
}

/// For custom providers: validates `request` against `capabilities` (or `null`).
#[uniffi::export]
pub fn validate(request: String, capabilities: String) -> String {
    letsdecide::json::validate(&request, &capabilities)
}

/// For custom providers: checks a canonical `response` against `request`.
#[uniffi::export]
pub fn check(request: String, response: String) -> String {
    letsdecide::json::check(&request, &response)
}

/// For custom providers: classifies a non-2xx reply; the error comes back as `ok`.
#[uniffi::export]
pub fn classify_http(response: String) -> String {
    letsdecide::json::classify_http(&response)
}
