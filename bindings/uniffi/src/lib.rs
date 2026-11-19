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
