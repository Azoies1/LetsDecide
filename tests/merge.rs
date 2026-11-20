//! Panels: `merge` combines member outcomes. See docs/concurrent-models.md.

use letsdecide::*;
use serde_json::{json, Value};

fn req() -> DecisionRequest {
    serde_json::from_value(json!({
        "input": "x",
        "questions": {
            "flag": { "kind": "yesno", "instructions": "?" },
            "topic": { "kind": "choice", "instructions": "?", "options": { "a": "A", "b": "B" } },
            "sev": { "kind": "score", "instructions": "?", "levels": [
                { "id": "low", "description": "" }, { "id": "mid", "description": "" }, { "id": "high", "description": "" } ] }
        }
    }))
    .unwrap()
}

/// One member's response: P(yes), chosen option, most likely level.
fn res(p: f32, choice: &str, level: &str, source: &str) -> DecisionResponse {
    let lv = |l: &str| if l == level { 0.8 } else { 0.1 };
    serde_json::from_value(json!({
        "model": null, "input_tokens": 10, "probability_source": source,
        "answers": {
            "flag": { "kind": "yesno", "probability": p },
            "topic": { "kind": "choice", "choice": choice, "confidence": null,
                       "probabilities": { "a": if choice == "a" { 0.9 } else { 0.1 }, "b": if choice == "b" { 0.9 } else { 0.1 } } },
            "sev": { "kind": "score", "value": 1.0, "confidence": null,
                     "probabilities": { "low": lv("low"), "mid": lv("mid"), "high": lv("high") } }
        }
    }))
    .unwrap()
}

fn ok(model: &str, weight: f32, r: DecisionResponse) -> Member {
    Member {
        model: model.into(),
        weight,
        outcome: Outcome::Answered(r),
    }
}

fn yes_p(m: &Merged) -> f32 {
    match m.response.answers["flag"] {
        Answer::YesNo { probability } => probability,
        _ => panic!(),
    }
}

#[test]
fn unanimous_vote() {
    let m = merge(
        &req(),
        &[
            ok("jev/a", 1.0, res(0.9, "a", "high", "provider")),
            ok("gemini/b", 1.0, res(0.7, "a", "high", "provider")),
        ],
        &Rules::default(),
    )
    .unwrap();
    assert_eq!(yes_p(&m), 1.0);
    assert!(m.disagreed.is_empty());
    assert_eq!(m.agreement["topic"], 1.0);
    assert_eq!(m.response.probability_source, ProbabilitySource::Vote);
    assert_eq!(m.response.input_tokens, Some(20));
    match &m.response.answers["sev"] {
        // Winner's 0-based level index, never an average of members' values.
        Answer::Score { value, .. } => assert_eq!(*value, 2.0),
        _ => panic!(),
    }
    m.response.check(&req()).unwrap();
}

#[test]
fn weighted_split_flags_disagreement() {
    let m = merge(
        &req(),
        &[
            ok("jev/a", 1.0, res(0.9, "a", "low", "provider")),
            ok("gemini/b", 2.0, res(0.2, "b", "mid", "provider")),
        ],
        &Rules::default(),
    )
    .unwrap();
    assert!((yes_p(&m) - 1.0 / 3.0).abs() < 1e-6);
    match &m.response.answers["topic"] {
        Answer::Choice { choice, .. } => assert_eq!(choice, "b"),
        _ => panic!(),
    }
    assert_eq!(m.disagreed, vec!["flag", "sev", "topic"]);
    assert!((m.agreement["topic"] - 2.0 / 3.0).abs() < 1e-6);
}

#[test]
fn ties_go_to_the_first_member() {
    let m = merge(
        &req(),
        &[
            ok("jev/a", 1.0, res(0.9, "b", "low", "provider")),
            ok("gemini/b", 1.0, res(0.9, "a", "low", "provider")),
        ],
        &Rules::default(),
    )
    .unwrap();
    match &m.response.answers["topic"] {
        Answer::Choice { choice, .. } => assert_eq!(choice, "b"),
        _ => panic!(),
    }
}

#[test]
fn refusals_abstain() {
    let mut refused = res(0.9, "a", "low", "provider");
    for a in refused.answers.values_mut() {
        *a = Answer::Refused;
    }
    refused.block_reason = Some("SAFETY".into());
    let one = merge(
        &req(),
        &[
            ok("gemini/b", 5.0, refused.clone()),
            ok("jev/a", 1.0, res(0.1, "a", "low", "provider")),
        ],
        &Rules::default(),
    )
    .unwrap();
    assert_eq!(yes_p(&one), 0.0); // the refusing member's weight doesn't count
    assert!(one.disagreed.is_empty());

    let all = merge(&req(), &[ok("gemini/b", 1.0, refused)], &Rules::default()).unwrap();
    assert!(all.response.answers.values().all(|a| *a == Answer::Refused));
    assert_eq!(all.response.block_reason.as_deref(), Some("SAFETY"));
}

#[test]
fn mean_averages_same_source_only() {
    let rules = Rules {
        merge: Method::Mean,
        ..Default::default()
    };
    let m = merge(
        &req(),
        &[
            ok("jev/a", 1.0, res(0.9, "a", "low", "provider")),
            ok("gemini/b", 1.0, res(0.5, "a", "low", "provider")),
        ],
        &rules,
    )
    .unwrap();
    assert!((yes_p(&m) - 0.7).abs() < 1e-6);
    assert_eq!(m.response.probability_source, ProbabilitySource::Provider);

    let e = merge(
        &req(),
        &[
            ok("jev/a", 1.0, res(0.9, "a", "low", "provider")),
            ok("local/b", 1.0, res(0.5, "a", "low", "logprob")),
        ],
        &rules,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported);
    let d = e.details.unwrap();
    assert_eq!(d.reason, Reason::MixedProbabilitySource);
    assert_eq!(d.provider.as_deref(), Some("local"));
}

#[test]
fn below_quorum_keeps_the_members_error() {
    let mut limited = Error::new(ErrorCode::RateLimited, "HTTP 429");
    limited.retry_after_ms = Some(2000);
    let e = merge(
        &req(),
        &[
            ok("jev/a", 1.0, res(0.9, "a", "low", "provider")),
            Member {
                model: "gemini/b".into(),
                weight: 1.0,
                outcome: Outcome::Failed(limited),
            },
            Member {
                model: "clef/c".into(),
                weight: 1.0,
                outcome: Outcome::Skipped,
            },
        ],
        &Rules {
            quorum: 2,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert!(e.retryable());
    assert_eq!(e.retry_after_ms, Some(2000));
    let d = e.details.unwrap();
    assert_eq!(d.provider.as_deref(), Some("gemini"));
    assert_eq!(
        (d.reason, d.limit, d.actual),
        (Reason::BelowQuorum, Some(2), Some(1))
    );
}

#[test]
fn malformed_member_counts_as_failed() {
    let mut bad = res(0.9, "a", "low", "provider");
    bad.answers.remove("topic");
    let e = merge(&req(), &[ok("acme/x", 1.0, bad)], &Rules::default()).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadResponse);
    assert_eq!(e.details.unwrap().provider.as_deref(), Some("acme"));
}

#[test]
fn rejects_bad_rules() {
    let m = [ok("jev/a", 0.0, res(0.9, "a", "low", "provider"))];
    assert_eq!(
        merge(&req(), &m, &Rules::default())
            .unwrap_err()
            .details
            .unwrap()
            .field
            .as_deref(),
        Some("weight")
    );
    let e = merge(
        &req(),
        &m,
        &Rules {
            quorum: 0,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(e.details.unwrap().field.as_deref(), Some("quorum"));
}

#[test]
fn json_entry_point() {
    let request = serde_json::to_string(&req()).unwrap();
    let r = serde_json::to_value(res(0.9, "a", "low", "provider")).unwrap();
    let members = json!([
        { "model": "jev/a", "response": r },
        { "model": "gemini/b", "error": { "code": "ProviderUnavailable", "message": "CANARY" } },
        { "model": "clef/c", "skipped": true }
    ]);
    let out: Value =
        serde_json::from_str(&json::merge(&request, &members.to_string(), "{}")).unwrap();
    assert_eq!(out["ok"]["answers"]["flag"]["probability"], 1.0);
    assert_eq!(out["ok"]["probability_source"], "vote");
    assert_eq!(out["ok"]["agreement"]["topic"], 1.0);

    let out = json::merge(&request, &members.to_string(), r#"{"quorum": 2}"#);
    assert!(!out.contains("CANARY"), "host messages are never echoed");
    let out: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(out["error"]["code"], "ProviderUnavailable");
    assert_eq!(out["error"]["retryable"], true);
    assert_eq!(out["error"]["details"]["reason"], "below_quorum");

    let both = json!([{ "model": "jev/a", "response": r, "skipped": true }]);
    let out: Value = serde_json::from_str(&json::merge(&request, &both.to_string(), "{}")).unwrap();
    assert_eq!(out["error"]["details"]["field"], "members");
}
