//! Python wrapper over `letsdecide::json`.

use pyo3::prelude::*;

#[pyfunction]
fn build(model: &str, request: &str, config: &str) -> String {
    letsdecide::json::build(model, request, config)
}

#[pyfunction]
fn parse(model: &str, request: &str, response: &str) -> String {
    letsdecide::json::parse(model, request, response)
}

#[pyfunction]
fn builtins() -> String {
    letsdecide::json::builtins()
}

#[pyfunction]
fn capabilities(model: &str) -> String {
    letsdecide::json::capabilities(model)
}

#[pyfunction]
fn validate(request: &str, capabilities: &str) -> String {
    letsdecide::json::validate(request, capabilities)
}

#[pyfunction]
fn check(request: &str, response: &str) -> String {
    letsdecide::json::check(request, response)
}

#[pyfunction]
fn classify_http(response: &str) -> String {
    letsdecide::json::classify_http(response)
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(build, m)?)?;
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(builtins, m)?)?;
    m.add_function(wrap_pyfunction!(capabilities, m)?)?;
    m.add_function(wrap_pyfunction!(validate, m)?)?;
    m.add_function(wrap_pyfunction!(check, m)?)?;
    m.add_function(wrap_pyfunction!(classify_http, m)?)?;
    Ok(())
}
