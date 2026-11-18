//! Golden fixtures: canonical request → exact HTTP request, and a recorded
//! response → canonical result (or error). Run every `fixtures/**/*.json`.

use letsdecide::*;
use serde_json::Value;
use std::fs;

fn fixtures() -> Vec<(String, Value)> {
    let mut out = vec![];
    for dir in fs::read_dir("tests/fixtures").unwrap() {
        for f in fs::read_dir(dir.unwrap().path()).unwrap() {
            let path = f.unwrap().path();
            let json = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            out.push((path.display().to_string(), json));
        }
    }
    out
}

#[test]
fn golden() {
    let all = fixtures();
    assert!(!all.is_empty());
    for (name, fx) in all {
        let model = fx["model"].as_str().unwrap();
        let cfg: Config = serde_json::from_value(fx["config"].clone()).unwrap();
        let req: DecisionRequest = serde_json::from_value(fx["request"].clone()).unwrap();
        let (provider, model) = resolve(model).unwrap();

        let http = provider.build(model, &req, &cfg).unwrap();
        let mut got = serde_json::to_value(&http).unwrap();
        got["body"] = serde_json::from_slice(&http.body).unwrap();
        got.as_object_mut().unwrap().remove("method");
        assert_eq!(got, fx["http"], "{name}: built request");

        let r = &fx["response"];
        let res = HttpResponse {
            status: r["status"].as_u64().unwrap() as u16,
            headers: serde_json::from_value(r["headers"].clone()).unwrap(),
            body: serde_json::to_vec(&r["body"]).unwrap(),
        };
        match (provider.parse(&req, &res), &fx["expected"]) {
            (Ok(got), e) => {
                let want: DecisionResponse = serde_json::from_value(e["ok"].clone()).unwrap();
                assert_eq!(got, want, "{name}: parsed response");
            }
            (Err(got), e) => {
                assert_eq!(
                    format!("{:?}", got.code),
                    e["error"]["code"].as_str().unwrap(),
                    "{name}"
                );
                assert_eq!(
                    got.retry_after_ms,
                    e["error"]["retry_after_ms"].as_u64(),
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn rejects_invalid_requests() {
    let req: DecisionRequest = serde_json::from_value(serde_json::json!({
        "input": "x",
        "questions": { "Bad-Id": { "kind": "yesno", "instructions": "?" } }
    }))
    .unwrap();
    let (p, m) = resolve("jev/jev-latest").unwrap();
    assert_eq!(
        p.build(m, &req, &Config::default()).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn clef_enforces_question_limit() {
    let questions = (0..65).map(|i| {
        (
            format!("q{i}"),
            serde_json::json!({ "kind": "yesno", "instructions": "?" }),
        )
    });
    let req: DecisionRequest =
        serde_json::from_value(serde_json::json!({ "input": "x", "questions": questions.collect::<serde_json::Map<_, _>>() })).unwrap();
    let cfg = Config {
        account_id: Some("a".into()),
        ..Default::default()
    };
    let (p, m) = resolve("clef/clef").unwrap();
    assert_eq!(
        p.build(m, &req, &cfg).unwrap_err().code,
        ErrorCode::Unsupported
    );
}

#[test]
fn unknown_provider_and_bad_ref() {
    assert_eq!(
        resolve("nope/x").err().unwrap().code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        resolve("nomodel").err().unwrap().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn config_debug_redacts_key() {
    let cfg = Config {
        api_key: Some("secret".into()),
        ..Default::default()
    };
    assert!(!format!("{cfg:?}").contains("secret"));
}
