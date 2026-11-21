//! Cost estimates from `spec/prices.json`. Prices change: treat the result as
//! an estimate and the file as the place to update them.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Prices {
    models: BTreeMap<String, Price>,
}

#[derive(Deserialize)]
struct Price {
    usd_per_million_input_tokens: f64,
}

fn prices() -> &'static BTreeMap<String, Price> {
    static P: OnceLock<BTreeMap<String, Price>> = OnceLock::new();
    P.get_or_init(|| {
        #[allow(clippy::expect_used)] // embedded data, loaded by tests
        let p: Prices = serde_json::from_str(include_str!("../spec/prices.json"))
            .expect("spec/prices.json is valid");
        p.models
    })
}

/// Estimated USD for `input_tokens` on `"provider/model"`, or `None` when the
/// price is unknown (Jev, custom servers, unlisted models).
pub fn estimate_cost_usd(model_ref: &str, input_tokens: u32) -> Option<f64> {
    let p = prices().get(model_ref)?;
    Some(f64::from(input_tokens) * p.usd_per_million_input_tokens / 1e6)
}
