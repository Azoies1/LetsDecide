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
        api_key: Some("k".into()),
        ..Default::default()
    };
    let (p, m) = resolve("clef/clef").unwrap();
    assert_eq!(
        p.build(m, &req, &cfg).unwrap_err().code,
        ErrorCode::Unsupported
    );
}

#[test]
fn jev_and_clef_cap_score_levels_at_ten() {
    let levels: Vec<_> = (0..11)
        .map(|i| serde_json::json!({ "id": format!("l{i}"), "description": "d" }))
        .collect();
    let req: DecisionRequest = serde_json::from_value(serde_json::json!({
        "input": "x",
        "questions": { "s": { "kind": "score", "instructions": "?", "levels": levels } }
    }))
    .unwrap();
    let cfg = Config {
        account_id: Some("a".into()),
        api_key: Some("k".into()),
        ..Default::default()
    };
    for model in ["jev/jev-latest", "clef/clef"] {
        let (p, m) = resolve(model).unwrap();
        assert_eq!(
            p.build(m, &req, &cfg).unwrap_err().code,
            ErrorCode::Unsupported,
            "{model}"
        );
    }
    let (p, m) = resolve("gemini/gemini-decision-1").unwrap();
    assert!(p.build(m, &req, &cfg).is_ok());
}

#[test]
fn image_limits() {
    let img = serde_json::json!({ "mime_type": "image/png", "data": "QUJD" });
    let mk = |n: usize, mime: &str| -> DecisionRequest {
        let mut i = img.clone();
        i["mime_type"] = mime.into();
        serde_json::from_value(serde_json::json!({
            "input": "x",
            "media": vec![i; n],
            "questions": { "q": { "kind": "yesno", "instructions": "?" } }
        }))
        .unwrap()
    };
    let cfg = Config {
        account_id: Some("a".into()),
        api_key: Some("k".into()),
        base_url: Some("http://h".into()),
        ..Default::default()
    };
    let code = |model: &str, req: &DecisionRequest| {
        let (p, m) = resolve(model).unwrap();
        p.build(m, req, &cfg).err().map(|e| e.code)
    };
    assert_eq!(
        code("clef/clef", &mk(5, "image/png")),
        Some(ErrorCode::Unsupported)
    );
    assert_eq!(
        code("clef/clef", &mk(1, "image/gif")),
        Some(ErrorCode::Unsupported)
    );
    assert_eq!(
        code("jev/jev-preview", &mk(1, "image/webp")),
        Some(ErrorCode::Unsupported)
    );
    assert_eq!(
        code("systemone/m", &mk(1, "image/png")),
        Some(ErrorCode::Unsupported)
    );
    assert_eq!(code("clef/clef", &mk(4, "image/webp")), None);
}

#[test]
fn non_image_media_is_gemini_only() {
    let req: DecisionRequest = serde_json::from_value(serde_json::json!({
        "input": "x",
        "media": [{ "mime_type": "audio/mp3", "data": "QUJD" }],
        "questions": { "q": { "kind": "yesno", "instructions": "?" } }
    }))
    .unwrap();
    let cfg = Config {
        account_id: Some("a".into()),
        api_key: Some("k".into()),
        base_url: Some("http://h".into()),
        ..Default::default()
    };
    for model in [
        "openai/gpt-6-luna",
        "clef/clef",
        "jev/jev-preview",
        "systemone/m",
    ] {
        let (p, m) = resolve(model).unwrap();
        assert_eq!(
            p.build(m, &req, &cfg).unwrap_err().code,
            ErrorCode::Unsupported,
            "{model}"
        );
    }
    let (p, m) = resolve("gemini/gemini-decision-1").unwrap();
    assert!(p.build(m, &req, &cfg).is_ok());
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

fn keyed() -> Config {
    Config {
        api_key: Some("k".into()),
        ..Default::default()
    }
}

#[test]
fn config_debug_redacts_key() {
    let cfg = Config {
        api_key: Some("secret".into()),
        ..Default::default()
    };
    assert!(!format!("{cfg:?}").contains("secret"));
}

#[test]
fn gemini_limits() {
    let cfg = keyed();
    let (p, m) = resolve("gemini/gemini-decision-1").unwrap();
    let opts: serde_json::Map<String, Value> = (0..256)
        .map(|i| (format!("o{i}"), Value::String("d".into())))
        .collect();
    let mk = |q: Value, imgs: Value| -> DecisionRequest {
        serde_json::from_value(serde_json::json!({
            "input": "x", "media": imgs, "questions": { "q": q }
        }))
        .unwrap()
    };
    let choice = mk(
        serde_json::json!({ "kind": "choice", "instructions": "?", "options": opts }),
        serde_json::json!([]),
    );
    assert_eq!(
        p.build(m, &choice, &cfg).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let yn = serde_json::json!({ "kind": "yesno", "instructions": "?" });
    let gif = serde_json::json!([{ "mime_type": "image/gif", "data": "QUJD" }]);
    let svg = serde_json::json!([{ "mime_type": "image/svg+xml", "data": "QUJD" }]);
    assert!(p.build(m, &mk(yn.clone(), gif), &cfg).is_ok());
    assert_eq!(
        p.build(m, &mk(yn, svg), &cfg).unwrap_err().code,
        ErrorCode::Unsupported
    );
}

fn gemini_build(req: serde_json::Value) -> Result<serde_json::Value, Error> {
    let req: DecisionRequest = serde_json::from_value(req).unwrap();
    let (p, m) = resolve("gemini/gemini-3-flash").unwrap();
    let http = p.build(m, &req, &keyed())?;
    Ok(serde_json::from_slice(&http.body).unwrap())
}

#[test]
fn gemini_media_options_and_noul_criteria() {
    let body = gemini_build(serde_json::json!({
        "input": "x",
        "media": [{ "mime_type": "video/mp4", "data": "AAAA",
                    "options": { "fps": 2, "start_offset": "1s" } }],
        "questions": { "q": { "kind": "yesno", "instructions": "?",
                              "yes_description": "is safe", "no_description": "is not" } }
    }))
    .unwrap();
    assert_eq!(body["input"][1]["fps"], 2);
    assert_eq!(body["input"][1]["type"], "video");
    assert_eq!(
        body["questions"]["q"]["criteria"]["true_description"],
        "is safe"
    );
    assert_eq!(
        body["questions"]["q"]["criteria"]["false_description"],
        "is not"
    );
}

#[test]
fn gemini_office_docs_are_not_capped_as_pdf() {
    let big = "A".repeat(70 * 1024 * 1024 * 4 / 3);
    gemini_build(serde_json::json!({
        "input": "x",
        "media": [{ "mime_type": "text/csv", "data": big }],
        "questions": { "q": { "kind": "yesno", "instructions": "?" } }
    }))
    .unwrap();
}

#[test]
fn gemini_enforces_pdf_size() {
    let big = "A".repeat(70 * 1024 * 1024 * 4 / 3);
    let err = gemini_build(serde_json::json!({
        "input": "x",
        "media": [{ "mime_type": "application/pdf", "data": big }],
        "questions": { "q": { "kind": "yesno", "instructions": "?" } }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unsupported);
}

#[test]
fn other_providers_reject_gemini_extras() {
    let req: DecisionRequest = serde_json::from_value(serde_json::json!({
        "input": "x",
        "questions": { "q": { "kind": "yesno", "instructions": "?", "yes_description": "y" } }
    }))
    .unwrap();
    for m in ["jev/jev-latest", "openai/gpt-5"] {
        let (p, m) = resolve(m).unwrap();
        assert_eq!(
            p.build(m, &req, &keyed()).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
}

#[test]
fn gemini_uri_media_and_wider_documents() {
    let body = gemini_build(serde_json::json!({
        "input": "x",
        "media": [
            { "mime_type": "text/csv", "uri": "https://example.com/a.csv" },
            { "mime_type": "image/png", "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc",
              "options": { "resolution": "high" } }
        ],
        "questions": { "Q-1": { "kind": "yesno", "instructions": "?" } }
    }))
    .unwrap();
    assert_eq!(body["input"][1]["type"], "document");
    assert_eq!(body["input"][1]["uri"], "https://example.com/a.csv");
    assert!(body["input"][1].get("data").is_none());
    assert_eq!(body["input"][2]["resolution"], "high");
    assert!(body["questions"].get("Q-1").is_some());
}

#[test]
fn gemini_rejects_bad_uris_and_missing_source() {
    for uri in [
        "gs://b/o",
        "https://www.youtube.com/watch?v=1",
        "https://youtu.be/x",
        "http://x.com/a.png",
    ] {
        let err = gemini_build(serde_json::json!({
            "input": "x",
            "media": [{ "mime_type": "image/png", "uri": uri }],
            "questions": { "q": { "kind": "yesno", "instructions": "?" } }
        }))
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Unsupported, "{uri}");
    }
    let err = gemini_build(serde_json::json!({
        "input": "x",
        "media": [{ "mime_type": "image/png" }],
        "questions": { "q": { "kind": "yesno", "instructions": "?" } }
    }))
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
}

#[test]
fn gemini_wide_ids() {
    gemini_build(serde_json::json!({
        "input": "x",
        "questions": { "q": { "kind": "choice", "instructions": "?",
                              "options": { "Option-A": "a", "b-2": "b" } } }
    }))
    .unwrap();
}

#[test]
fn other_providers_reject_uri_media() {
    let req: DecisionRequest = serde_json::from_value(serde_json::json!({
        "input": "x",
        "media": [{ "mime_type": "image/png", "uri": "https://example.com/a.png" }],
        "questions": { "q": { "kind": "yesno", "instructions": "?" } }
    }))
    .unwrap();
    let (p, m) = resolve("jev/jev-latest").unwrap();
    assert_eq!(
        p.build(m, &req, &keyed()).unwrap_err().code,
        ErrorCode::Unsupported
    );
}

fn clef_http(cfg: Config) -> Result<HttpRequest, Error> {
    let req: DecisionRequest = serde_json::from_value(serde_json::json!({
        "input": "x",
        "questions": { "q": { "kind": "yesno", "instructions": "?" } }
    }))
    .unwrap();
    let (p, m) = resolve("clef/clef-flash").unwrap();
    p.build(m, &req, &cfg)
}

fn header<'a>(h: &'a HttpRequest, name: &str) -> Option<&'a str> {
    h.headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[test]
fn clef_oauth_token_is_bearer_and_wins_over_key() {
    let http = clef_http(Config {
        account_id: Some("a".into()),
        access_token: Some("tok".into()),
        api_key: Some("key".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(header(&http, "authorization"), Some("Bearer tok"));
}

#[test]
fn clef_still_accepts_api_key_alone() {
    let http = clef_http(Config {
        account_id: Some("a".into()),
        api_key: Some("key".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(header(&http, "authorization"), Some("Bearer key"));
}

#[test]
fn clef_without_any_credential_fails_before_sending() {
    let err = clef_http(Config {
        account_id: Some("a".into()),
        ..Default::default()
    })
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
}

#[test]
fn gemini_sends_key_header_and_requires_it() {
    let req: DecisionRequest = serde_json::from_value(serde_json::json!({
        "input": "x",
        "questions": { "q": { "kind": "yesno", "instructions": "?" } }
    }))
    .unwrap();
    let (p, m) = resolve("gemini/gemini-decision-1").unwrap();
    let http = p.build(m, &req, &keyed()).unwrap();
    assert_eq!(header(&http, "x-goog-api-key"), Some("k"));
    assert!(header(&http, "authorization").is_none());
    let err = p.build(m, &req, &Config::default()).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
}

#[test]
fn config_debug_redacts_access_token() {
    let cfg = Config {
        access_token: Some("secret-token".into()),
        ..Default::default()
    };
    assert!(!format!("{cfg:?}").contains("secret-token"));
}

fn build_json(model: &str, req: serde_json::Value) -> Result<serde_json::Value, Error> {
    let req: DecisionRequest = serde_json::from_value(req).unwrap();
    let (p, m) = resolve(model).unwrap();
    let http = p.build(m, &req, &keyed())?;
    Ok(serde_json::from_slice(&http.body).unwrap())
}

#[test]
fn openai_image_urls_must_be_public_http_images() {
    for (media, code) in [
        (
            serde_json::json!({ "mime_type": "image/png", "uri": "gs://b/o" }),
            ErrorCode::Unsupported,
        ),
        (
            serde_json::json!({ "mime_type": "application/pdf", "uri": "https://x/a.pdf" }),
            ErrorCode::Unsupported,
        ),
        (
            serde_json::json!({ "mime_type": "image/png", "uri": "https://x/a.png", "data": "QQ==" }),
            ErrorCode::InvalidRequest,
        ),
    ] {
        let err = build_json(
            "openai/gpt-6-luna",
            serde_json::json!({
                "input": "x", "media": [media],
                "questions": { "q": { "kind": "yesno", "instructions": "?" } }
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, code);
    }
    let body = build_json(
        "openai/gpt-6-luna",
        serde_json::json!({
            "input": "x", "media": [{ "mime_type": "image/png", "uri": "http://x/a.png" }],
            "questions": { "q": { "kind": "yesno", "instructions": "?" } }
        }),
    )
    .unwrap();
    assert_eq!(
        body["input"][0]["content"][1]["image_url"],
        "http://x/a.png"
    );
}

#[test]
fn provider_extras_are_merged_but_never_overwrite() {
    for model in [
        "openai/gpt-6-luna",
        "jev/jev-latest",
        "gemini/gemini-decision-1",
    ] {
        let body = build_json(
            model,
            serde_json::json!({
                "input": "x", "provider_extras": { "service_tier": "flex", "metadata": { "a": 1 } },
                "questions": { "q": { "kind": "yesno", "instructions": "?" } }
            }),
        )
        .unwrap();
        assert_eq!(body["service_tier"], "flex", "{model}");
        assert_eq!(body["metadata"]["a"], 1, "{model}");
        let err = build_json(
            model,
            serde_json::json!({
                "input": "x", "provider_extras": { "model": "other" },
                "questions": { "q": { "kind": "yesno", "instructions": "?" } }
            }),
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest, "{model}");
    }
}

#[test]
fn confidence_is_read_off_probabilities_when_not_sent() {
    let req: DecisionRequest = serde_json::from_value(serde_json::json!({
        "input": "x",
        "questions": {
            "c": { "kind": "choice", "instructions": "?", "options": { "a": "A", "b": "B" } },
            "s": { "kind": "score", "instructions": "?",
                   "levels": [{ "id": "lo", "description": "l" }, { "id": "hi", "description": "h" }] }
        }
    }))
    .unwrap();
    let body = serde_json::json!({ "answers": {
        "c": { "choice": "b", "probabilities": { "a": 0.3, "b": 0.7 } },
        "s": { "score": 0.6, "probabilities": { "0": 0.4, "1": 0.6 } }
    }});
    let (p, _) = resolve("jev/jev-latest").unwrap();
    let res = p
        .parse(
            &req,
            &HttpResponse {
                status: 200,
                headers: vec![],
                body: serde_json::to_vec(&body).unwrap(),
            },
        )
        .unwrap();
    let out = serde_json::to_value(&res).unwrap();
    assert_eq!(out["answers"]["c"]["confidence"], 0.7f32 as f64);
    assert_eq!(out["answers"]["c"]["confidence_source"], "probabilities");
    assert_eq!(out["answers"]["s"]["confidence"], 0.6f32 as f64);
    assert_eq!(out["answers"]["s"]["confidence_source"], "probabilities");
}
