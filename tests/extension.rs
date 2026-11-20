//! Registry, capabilities, response checks, config headers, and the rule
//! that errors never carry sensitive data.

use letsdecide::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn req(v: Value) -> DecisionRequest {
    serde_json::from_value(v).unwrap()
}

fn yesno() -> DecisionRequest {
    req(json!({ "input": "x", "questions": { "q": { "kind": "yesno", "instructions": "?" } } }))
}

/// A custom provider: fixed answers, tiny limits.
struct Acme {
    reply: Option<DecisionResponse>,
}

impl Provider for Acme {
    fn capabilities(&self, _model: &str) -> Capabilities {
        Capabilities {
            max_questions: Some(2),
            question_kinds: vec!["yesno".into(), "choice".into()],
            locality: Locality::Device,
            ..Default::default()
        }
    }
    fn build(&self, model: &str, _: &DecisionRequest, cfg: &Config) -> Result<HttpRequest, Error> {
        let mut http = HttpRequest {
            method: "POST",
            url: format!("https://acme.test/{model}"),
            headers: vec![],
            body: vec![],
        };
        apply_headers(&mut http, cfg)?;
        Ok(http)
    }
    fn parse(&self, _: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error> {
        if res.status >= 400 {
            return Err(http_error(res));
        }
        Ok(self.reply.clone().unwrap())
    }
}

fn ok_response() -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![],
        body: vec![],
    }
}

fn answers(a: Answer) -> DecisionResponse {
    DecisionResponse {
        model: None,
        answers: BTreeMap::from([("q".to_owned(), a)]),
        input_tokens: None,
        block_reason: None,
        probability_source: ProbabilitySource::Logprob,
    }
}

#[test]
fn registry_registers_custom_providers() {
    let mut r = Registry::builtin();
    r.register("acme", Acme { reply: None }).unwrap();
    assert!(r.names().any(|n| n == "acme"));
    let http = r.build("acme/m1", &yesno(), &Config::default()).unwrap();
    assert_eq!(http.url, "https://acme.test/m1");
    assert_eq!(
        r.capabilities("acme/m1").unwrap().locality,
        Locality::Device
    );
}

#[test]
fn registry_refuses_taken_names_unless_replaced() {
    let mut r = Registry::builtin();
    for name in BUILTIN {
        let e = r.register(name, Acme { reply: None }).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidRequest);
        let d = e.details.unwrap();
        assert_eq!(d.reason, Reason::NameTaken);
        assert_eq!(d.provider.as_deref(), Some(name));
    }
    r.register("acme", Acme { reply: None }).unwrap();
    assert!(r.register("acme", Acme { reply: None }).is_err());
    assert!(r.replace("openai", Acme { reply: None }).unwrap().is_some());
    assert_eq!(
        r.build("openai/x", &yesno(), &Config::default())
            .unwrap()
            .url,
        "https://acme.test/x"
    );
}

#[test]
fn registry_rejects_bad_names() {
    let mut r = Registry::empty();
    for name in ["", "Acme", "a/b", "a b", &"a".repeat(65)] {
        assert_eq!(
            r.register(name, Acme { reply: None }).unwrap_err().code,
            ErrorCode::InvalidRequest,
            "{name:?}"
        );
    }
    r.register("my-corp_2", Acme { reply: None }).unwrap();
}

#[test]
fn unknown_provider_has_details() {
    let e = Registry::empty().resolve("acme/m").err().unwrap();
    assert_eq!(e.code, ErrorCode::Unsupported);
    let d = e.details.unwrap();
    assert_eq!(d.reason, Reason::UnknownProvider);
    assert_eq!(d.provider.as_deref(), Some("acme"));
}

#[test]
fn capabilities_are_checked_before_build_with_details() {
    let mut r = Registry::empty();
    r.register("acme", Acme { reply: None }).unwrap();
    let cfg = Config::default();

    let three = req(json!({ "input": "x", "questions": {
        "a": { "kind": "yesno", "instructions": "?" },
        "b": { "kind": "yesno", "instructions": "?" },
        "c": { "kind": "yesno", "instructions": "?" } } }));
    let d = r
        .build("acme/m", &three, &cfg)
        .unwrap_err()
        .details
        .unwrap();
    assert_eq!(
        (d.reason, d.limit, d.actual, d.provider.as_deref()),
        (Reason::TooManyQuestions, Some(2), Some(3), Some("acme"))
    );

    let score = req(
        json!({ "input": "x", "questions": { "s": { "kind": "score", "instructions": "?",
        "levels": [{ "id": "lo", "description": "" }, { "id": "hi", "description": "" }] } } }),
    );
    let d = r
        .build("acme/m", &score, &cfg)
        .unwrap_err()
        .details
        .unwrap();
    assert_eq!(
        (d.reason, d.question.as_deref()),
        (Reason::QuestionKind, Some("s"))
    );

    let mut img = yesno();
    img.media.push(Media {
        mime_type: "image/png".into(),
        data: "QUJD".into(),
        uri: None,
        options: Default::default(),
    });
    let d = r.build("acme/m", &img, &cfg).unwrap_err().details.unwrap();
    assert_eq!(
        (d.reason, d.mime_type.as_deref()),
        (Reason::MediaNotAccepted, Some("image/png"))
    );
}

#[test]
fn builtin_limits_have_details() {
    let levels: Vec<_> = (0..11)
        .map(|i| json!({ "id": format!("l{i}"), "description": "d" }))
        .collect();
    let r = req(
        json!({ "input": "x", "questions": { "s": { "kind": "score", "instructions": "?", "levels": levels } } }),
    );
    let e = Registry::builtin()
        .build("jev/jev-latest", &r, &Config::default())
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported);
    let d = e.details.unwrap();
    assert_eq!(d.reason, Reason::TooManyLevels);
    assert_eq!((d.limit, d.actual), (Some(10), Some(11)));
    assert_eq!(d.question.as_deref(), Some("s"));
    assert_eq!(d.provider.as_deref(), Some("jev"));
}

#[test]
fn builtin_capabilities() {
    let r = Registry::builtin();
    let gemini = r.capabilities("gemini/gemini-decision-1").unwrap();
    assert!(gemini.accepts_media("audio/mp3"));
    assert!(gemini.accepts_media("image/heic"));
    assert!(!gemini.accepts_media("image/svg+xml"));
    let openai = r.capabilities("openai/gpt-6-luna").unwrap();
    assert!(openai.accepts_media("image/gif"));
    assert!(!openai.accepts_media("audio/mp3"));
    assert_eq!(
        r.capabilities("systemone/m").unwrap().locality,
        Locality::Network
    );
    assert_eq!(r.capabilities("clef/x").unwrap().max_media, Some(4));
}

#[test]
fn response_check_catches_malformed_answers() {
    let choice = req(
        json!({ "input": "x", "questions": { "q": { "kind": "choice", "instructions": "?",
        "options": { "a": "A", "b": "B" } } } }),
    );
    let probs = |pairs: &[(&str, f32)]| -> BTreeMap<String, f32> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    };
    let c = |choice: &str, conf: Option<f32>, p: BTreeMap<String, f32>| {
        answers(Answer::Choice {
            choice: choice.into(),
            confidence: conf,
            confidence_source: None,
            probabilities: p,
        })
    };
    let reason = |res: DecisionResponse, req: &DecisionRequest| {
        let e = res.check(req).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadResponse);
        e.details.unwrap().reason
    };

    assert!(c("a", Some(0.7), probs(&[("a", 0.7), ("b", 0.3)]))
        .check(&choice)
        .is_ok());
    assert!(answers(Answer::Refused).check(&choice).is_ok());
    assert_eq!(reason(c("z", None, probs(&[])), &choice), Reason::UnknownId);
    assert_eq!(
        reason(c("a", None, probs(&[("z", 0.1)])), &choice),
        Reason::UnknownId
    );
    assert_eq!(
        reason(c("a", None, probs(&[("a", 1.5)])), &choice),
        Reason::InvalidNumber
    );
    assert_eq!(
        reason(c("a", Some(f32::NAN), probs(&[])), &choice),
        Reason::InvalidNumber
    );
    assert_eq!(
        reason(answers(Answer::YesNo { probability: 0.5 }), &choice),
        Reason::WrongAnswerKind
    );

    let yn = yesno();
    assert_eq!(
        reason(answers(Answer::YesNo { probability: -0.2 }), &yn),
        Reason::InvalidNumber
    );
    let mut missing = answers(Answer::Refused);
    missing.answers.clear();
    assert_eq!(reason(missing, &yn), Reason::MissingAnswer);
    let mut extra = answers(Answer::YesNo { probability: 0.5 });
    extra.answers.insert("other".into(), Answer::Refused);
    assert_eq!(reason(extra, &yn), Reason::UnexpectedAnswer);
}

#[test]
fn registry_parse_checks_custom_responses() {
    let mut r = Registry::empty();
    let bad = answers(Answer::YesNo { probability: 2.0 });
    r.register("acme", Acme { reply: Some(bad) }).unwrap();
    let e = r.parse("acme/m", &yesno(), &ok_response()).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadResponse);
    assert_eq!(e.details.unwrap().provider.as_deref(), Some("acme"));

    let good = answers(Answer::YesNo { probability: 0.2 });
    r.replace(
        "acme",
        Acme {
            reply: Some(good.clone()),
        },
    )
    .unwrap();
    assert_eq!(r.parse("acme/m", &yesno(), &ok_response()).unwrap(), good);
}

#[test]
fn config_headers_are_added_and_override() {
    let cfg: Config = serde_json::from_value(json!({
        "api_key": "k",
        "headers": [["x-gateway", "g1"], ["Authorization", "Token t"]]
    }))
    .unwrap();
    let (p, m) = resolve("jev/jev-latest").unwrap();
    let http = p.build(m, &yesno(), &cfg).unwrap();
    let auth: Vec<_> = http
        .headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .collect();
    assert_eq!(auth, [&("Authorization".to_owned(), "Token t".to_owned())]);
    assert!(http.headers.contains(&("x-gateway".into(), "g1".into())));
}

#[test]
fn config_headers_reject_injection() {
    for headers in [
        json!([["x-a", "ok\r\nx-evil: 1"]]),
        json!([["bad name", "v"]]),
    ] {
        let cfg: Config = serde_json::from_value(json!({ "headers": headers })).unwrap();
        let (p, m) = resolve("openai/gpt-6-luna").unwrap();
        let e = p.build(m, &yesno(), &cfg).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidRequest);
        assert_eq!(e.details.unwrap().field.as_deref(), Some("headers"));
    }
}

const CANARY: &str = "c4n4ry-s3cret";

/// Everything an error might be rendered as.
fn rendered(e: &Error) -> String {
    format!("{e} {e:?}")
}

#[test]
fn errors_never_carry_sensitive_data() {
    let c = CANARY;
    let cfg: Config = serde_json::from_value(json!({
        "api_key": c,
        "account_id": "acct",
        "base_url": format!("https://h.test/?token={c}"),
        "headers": [["x-tenant", c]],
        "options": { "secret": c }
    }))
    .unwrap();
    assert!(!format!("{cfg:?}").contains(c), "Config Debug");

    let levels: Vec<_> = (0..300)
        .map(|i| json!({ "id": format!("l{i}"), "description": c }))
        .collect();
    let options: serde_json::Map<String, Value> = (0..300)
        .map(|i| (format!("o{i}"), Value::String(c.into())))
        .collect();
    let questions: serde_json::Map<String, Value> = (0..1001)
        .map(|i| {
            (
                format!("q{i}"),
                json!({ "kind": "yesno", "instructions": c }),
            )
        })
        .collect();
    let media = |mime: &str| json!([{ "mime_type": mime, "data": c }]);
    let requests = [
        json!({ "input": c, "questions": questions }),
        json!({ "input": c, "questions": { "s": { "kind": "score", "instructions": c, "levels": levels } } }),
        json!({ "input": c, "questions": { "o": { "kind": "choice", "instructions": c, "options": options } } }),
        json!({ "input": c, "media": media("image/svg+xml"), "questions": { "q": { "kind": "yesno", "instructions": c } } }),
        json!({ "input": c, "media": media("audio/mp3"), "questions": { "q": { "kind": "yesno", "instructions": c } } }),
        json!({ "input": { "k": c }, "questions": { "q": { "kind": "choice", "instructions": c, "options": { "a": c } } } }),
    ];
    let r = Registry::builtin();
    let models = BUILTIN.map(|p| format!("{p}/m"));
    let mut seen = 0;
    for rq in &requests {
        let rq = req(rq.clone());
        for m in &models {
            if let Err(e) = r.build(m, &rq, &cfg) {
                seen += 1;
                assert!(!rendered(&e).contains(c), "{m}: {}", rendered(&e));
            }
        }
    }
    assert!(seen > 10, "expected many failures, got {seen}");

    // Response side: canary in error bodies and in malformed 200 bodies.
    let bodies = [
        json!({ "error": { "message": c } }).to_string(),
        format!("not json {c}"),
        json!({ "answers": { "q": { "noul": c } } }).to_string(),
        json!({ "answers": [ { "type": "predicate", "probability": c } ] }).to_string(),
        json!({ "answers": { "q": { "type": c } } }).to_string(),
    ];
    let yn = yesno();
    for body in &bodies {
        for status in [200, 400, 401, 429, 500] {
            for m in &models {
                let res = HttpResponse {
                    status,
                    headers: vec![("x-echo".into(), c.into())],
                    body: body.clone().into_bytes(),
                };
                if let Err(e) = r.parse(m, &yn, &res) {
                    assert!(!rendered(&e).contains(c), "{m} {status}: {}", rendered(&e));
                }
            }
        }
    }
}

#[test]
fn json_layer_errors_never_carry_sensitive_data() {
    let c = CANARY;
    let outs = [
        // Wrong types in the request: serde would quote the value.
        json::build(
            "jev/m",
            &json!({ "input": "x", "questions": { "q": { "kind": c } } }).to_string(),
            "{}",
        ),
        json::build(
            "jev/m",
            &json!({ "input": "x", "questions": c }).to_string(),
            "{}",
        ),
        json::build(
            "jev/m",
            &json!({ "input": "x", "questions": {} }).to_string(),
            &json!({ "api_key": 5, "options": c }).to_string(),
        ),
        json::build("jev/m", &format!("{{\"input\": \"{c}\""), "{}"),
        json::parse(
            "jev/m",
            &yesno_json(),
            &json!({ "status": 200, "headers": [], "body": format!("[{c}") }).to_string(),
        ),
        json::check(
            &yesno_json(),
            &json!({ "answers": { "q": { "kind": c } } }).to_string(),
        ),
        json::validate(&yesno_json(), &json!({ "max_questions": c }).to_string()),
        json::classify_http(
            &json!({ "status": 500, "headers": [["retry-after", c]], "body": c }).to_string(),
        ),
    ];
    for out in outs {
        assert!(!out.contains(c), "{out}");
    }
}

fn yesno_json() -> String {
    json!({ "input": "x", "questions": { "q": { "kind": "yesno", "instructions": "?" } } })
        .to_string()
}

#[test]
fn json_entry_points() {
    let caps: Value = serde_json::from_str(&json::capabilities("clef/x")).unwrap();
    assert_eq!(caps["ok"]["max_questions"], 64);

    let v: Value = serde_json::from_str(&json::validate(&yesno_json(), "null")).unwrap();
    assert_eq!(v, json!({ "ok": null }));
    let caps = json!({ "question_kinds": ["choice"] }).to_string();
    let v: Value = serde_json::from_str(&json::validate(&yesno_json(), &caps)).unwrap();
    assert_eq!(v["error"]["details"]["reason"], "question_kind");

    let res = json!({ "model": null, "input_tokens": null,
        "answers": { "q": { "kind": "yesno", "probability": 0.4 } } })
    .to_string();
    let v: Value = serde_json::from_str(&json::check(&yesno_json(), &res)).unwrap();
    assert_eq!(v["ok"]["probability_source"], "provider");

    let v: Value = serde_json::from_str(&json::classify_http(
        &json!({ "status": 429, "headers": [["Retry-After", "3"]], "body": "" }).to_string(),
    ))
    .unwrap();
    assert_eq!(v["ok"]["code"], "RateLimited");
    assert_eq!(v["ok"]["retry_after_ms"], 3000);
    assert_eq!(v["ok"]["retryable"], true);

    let v: Value = serde_json::from_str(&json::build("nope/m", &yesno_json(), "{}")).unwrap();
    assert_eq!(v["error"]["details"]["reason"], "unknown_provider");
    assert_eq!(v["error"]["details"]["provider"], "nope");
}

#[test]
fn registry_accepts_gemini_wide_ids() {
    let r = req(
        json!({ "input": "x", "questions": { "Is-Positive": { "kind": "yesno", "instructions": "?" } } }),
    );
    let cfg = Config {
        api_key: Some("k".into()),
        ..Default::default()
    };
    assert!(Registry::builtin()
        .build("gemini/gemini-decision-1", &r, &cfg)
        .is_ok());
    let e = Registry::builtin()
        .build("jev/jev-latest", &r, &cfg)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidRequest);
}
