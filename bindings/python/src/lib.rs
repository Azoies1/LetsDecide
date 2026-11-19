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

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(build, m)?)?;
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    Ok(())
}
