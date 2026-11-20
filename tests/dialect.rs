//! Dialects: built-ins as data, `extends`, validation, and providers that
//! differ from a built-in only in data.

use letsdecide::*;
use serde_json::{json, Value};

fn req(v: Value) -> DecisionRequest {
    serde_json::from_value(v).unwrap()
}

fn yesno() -> DecisionRequest {
    req(json!({ "input": "x", "questions": { "q": { "kind": "yesno", "instructions": "?" } } }))
}

fn cfg(v: Value) -> Config {
    serde_json::from_value(v).unwrap()
}

fn body(http: &HttpRequest) -> Value {
    serde_json::from_slice(&http.body).unwrap()
}

fn header<'a>(http: &'a HttpRequest, name: &str) -> Option<&'a str> {
    http.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn reason(e: &Error) -> Reason {
    e.details.as_ref().expect("details").reason
}

#[test]
fn every_builtin_is_a_valid_dialect() {
    for name in BUILTIN {
        let d = Dialect::builtin(name).unwrap_or_else(|| panic!("{name}"));
        d.validate().unwrap();
        let v = Dialect::builtin_json(name).unwrap();
        assert!(v.get("_doc").is_none(), "{name}: comments are stripped");
        // Extending it with nothing changes nothing; restating its family is fine.
        assert_eq!(
            Dialect::from_value(json!({ "extends": name })).unwrap(),
            d,
            "{name}"
        );
        let same = json!({ "extends": name, "family": v["family"].clone() });
        assert_eq!(Dialect::from_value(same).unwrap(), d, "{name}");
    }
    assert!(Dialect::builtin("nope").is_none());
}

#[test]
fn extends_builds_a_provider_that_differs_only_in_data() {
    // A Jev-shaped API at another host and path (e.g. a vendor that copied System One).
    let d = Dialect::from_value(json!({
        "extends": "jev",
        "base_url": "https://api.acme.test",
        "path": "/v1/decisions"
    }))
    .unwrap();
    let c = cfg(json!({ "api_key": "k" }));
    let http = d.build("acme-1", &yesno(), &c).unwrap();
    assert_eq!(http.url, "https://api.acme.test/v1/decisions");
    assert_eq!(header(&http, "authorization"), Some("Bearer k"));
    // Same body as Jev itself.
    let jev = Dialect::builtin("jev")
        .unwrap()
        .build("acme-1", &yesno(), &c)
        .unwrap();
    assert_eq!(body(&http), body(&jev));
}

#[test]
fn extends_merges_objects_and_keeps_the_rest() {
    let d = Dialect::from_value(json!({
        "extends": "systemone",
        "base_url": "https://foundry.test/providers/microsoft",
        "auth": { "header": "api-key" },
        "capabilities": { "max_questions": 20 }
    }))
    .unwrap();
    assert_eq!(d.path, "/v1/systemone"); // kept from the base
    assert_eq!(d.capabilities.max_questions, Some(20));
    assert_eq!(d.capabilities.locality, Locality::Network); // merged, not replaced
    let http = d
        .build("decision-1", &yesno(), &cfg(json!({ "api_key": "k" })))
        .unwrap();
    assert_eq!(
        http.url,
        "https://foundry.test/providers/microsoft/v1/systemone"
    );
    assert_eq!(header(&http, "api-key"), Some("k"));
    assert_eq!(header(&http, "authorization"), None);
}

#[test]
fn an_openai_compatible_endpoint_is_a_dialect() {
    let d = Dialect::from_value(json!({
        "extends": "openai",
        "base_url": "https://my-resource.openai.azure.test/openai",
        "auth": { "header": "api-key", "required": true }
    }))
    .unwrap();
    let http = d
        .build("gpt-6-luna", &yesno(), &cfg(json!({ "api_key": "k" })))
        .unwrap();
    assert_eq!(
        http.url,
        "https://my-resource.openai.azure.test/openai/v1/decisions"
    );
    assert_eq!(header(&http, "api-key"), Some("k"));
    assert_eq!(body(&http)["questions"][0]["type"], "predicate");
    let e = d
        .build("gpt-6-luna", &yesno(), &Config::default())
        .unwrap_err();
    assert_eq!(
        (e.code, reason(&e)),
        (ErrorCode::InvalidRequest, Reason::MissingConfig)
    );
}

#[test]
fn a_vendor_api_change_is_a_data_patch() {
    // Gemini moves to /v1/decisions: replace the built-in with a patched copy.
    let patched =
        Dialect::from_value(json!({ "extends": "gemini", "path": "/v1/decisions" })).unwrap();
    let mut r = Registry::builtin();
    r.replace("gemini", patched).unwrap();
    let http = r
        .build(
            "gemini/gemini-decision-2",
            &yesno(),
            &cfg(json!({ "api_key": "k" })),
        )
        .unwrap();
    assert_eq!(
        http.url,
        "https://generativelanguage.googleapis.com/v1/decisions"
    );
    assert_eq!(header(&http, "x-goog-api-key"), Some("k"));
}

#[test]
fn envelope_and_block_codes_are_data() {
    let d = Dialect::from_value(json!({
        "extends": "systemone",
        "base_url": "https://h.test",
        "envelope": "data",
        "blocked_error_codes": ["policy"]
    }))
    .unwrap();
    let ok = HttpResponse {
        status: 200,
        headers: vec![],
        body: json!({ "data": { "answers": { "q": { "noul": 0.25 } } } })
            .to_string()
            .into_bytes(),
    };
    assert_eq!(
        d.parse(&yesno(), &ok).unwrap().answers["q"],
        Answer::YesNo { probability: 0.25 }
    );
    let blocked = HttpResponse {
        status: 400,
        headers: vec![],
        body: json!({ "error": { "code": "policy", "message": "no" } })
            .to_string()
            .into_bytes(),
    };
    let out = d.parse(&yesno(), &blocked).unwrap();
    assert_eq!(out.answers["q"], Answer::Refused);
    assert_eq!(out.block_reason.as_deref(), Some("no"));
    let other = HttpResponse {
        body: json!({ "error": { "code": "bad_field" } })
            .to_string()
            .into_bytes(),
        ..blocked
    };
    assert_eq!(
        d.parse(&yesno(), &other).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn typos_and_bad_combinations_fail_at_registration() {
    let bad = |v: Value| Dialect::from_value(v).unwrap_err();
    let msg = |v: Value| bad(v).message;

    assert!(msg(json!({ "extends": "jev", "pathh": "/x" })).contains("unknown field `pathh`"));
    assert!(
        msg(json!({ "extends": "jev", "auth": { "header": "a", "scheme": "bearr" } }))
            .contains("unknown variant `bearr`")
    );
    assert!(msg(json!({ "base_url": "https://h", "path": "/x" })).contains("`extends`"));
    assert!(msg(json!({ "extends": "jev", "family": "openai" }))
        .contains("`family` comes from `extends`"));

    for v in [
        json!({ "extends": "nope" }),
        json!({ "extends": "jev", "path": "no-slash" }),
        json!({ "extends": "jev", "path": "/v1/{region}" }),
        json!({ "extends": "jev", "path": "/v1/{model" }),
        json!({ "extends": "jev", "base_url": "ftp://h" }),
        json!({ "extends": "jev", "auth": { "header": "bad header" } }),
        json!({ "extends": "jev", "media_headers": [["x", "y"]] }),
        json!({ "extends": "jev", "json_input": "text" }),
        json!({ "extends": "jev", "path": "/{options.Region}" }),
        json!({ "extends": "openai", "media_encoding": "jev_assets" }),
        json!({ "extends": "jev", "media_encoding": "content_parts" }),
        json!({ "extends": "systemone", "capabilities": { "media": ["image/png"] } }),
        json!("not an object"),
    ] {
        let e = bad(v.clone());
        assert_eq!(e.code, ErrorCode::InvalidRequest, "{v}");
        assert_eq!(reason(&e), Reason::InvalidConfig, "{v}");
    }
}

#[test]
fn path_values_stay_one_segment() {
    let clef = Dialect::builtin("clef").unwrap();
    let c = cfg(json!({ "api_key": "k", "account_id": "a" }));
    for model in ["../admin", "x/y", "a?b", "a#b", "a%2f", "a b", ".."] {
        let e = clef.build(model, &yesno(), &c).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidRequest, "{model}");
        assert_eq!(
            e.details.unwrap().field.as_deref(),
            Some("model"),
            "{model}"
        );
    }
    let e = clef
        .build(
            "m",
            &yesno(),
            &cfg(json!({ "api_key": "k", "account_id": "a/../b" })),
        )
        .unwrap_err();
    assert_eq!(e.details.unwrap().field.as_deref(), Some("account_id"));
    let e = clef
        .build("m", &yesno(), &cfg(json!({ "api_key": "k" })))
        .unwrap_err();
    assert_eq!(reason(&e), Reason::MissingConfig);
}

#[test]
fn base_url_is_required_when_the_dialect_has_none() {
    let e = Dialect::builtin("systemone")
        .unwrap()
        .build("m", &yesno(), &Config::default())
        .unwrap_err();
    assert_eq!(e.details.unwrap().field.as_deref(), Some("base_url"));
}

#[test]
fn media_headers_are_sent_only_with_media() {
    let jev = Dialect::builtin("jev").unwrap();
    let c = cfg(json!({ "api_key": "k" }));
    let plain = jev.build("jev-latest", &yesno(), &c).unwrap();
    assert_eq!(header(&plain, "typesafe-danger-zone-images"), None);
    let mut with_image = yesno();
    with_image.media.push(Media {
        mime_type: "image/png".into(),
        data: "QUJD".into(),
        uri: None,
        options: Default::default(),
    });
    let http = jev.build("jev-preview", &with_image, &c).unwrap();
    assert_eq!(header(&http, "typesafe-danger-zone-images"), Some("true"));
}

#[test]
fn json_layer_builds_and_parses_with_a_dialect() {
    let dialect =
        json!({ "extends": "jev", "base_url": "https://api.acme.test", "path": "/v1/decisions" });
    let out: Value = serde_json::from_str(&json::resolve_dialect(&dialect.to_string())).unwrap();
    assert_eq!(out["ok"]["path"], "/v1/decisions");
    assert_eq!(out["ok"]["family"], "systemone");
    let resolved = out["ok"].to_string();

    let request =
        json!({ "input": "x", "questions": { "q": { "kind": "yesno", "instructions": "?" } } })
            .to_string();
    let http: Value = serde_json::from_str(&json::build_dialect(
        &resolved,
        "acme-1",
        &request,
        &json!({ "api_key": "k" }).to_string(),
    ))
    .unwrap();
    assert_eq!(http["ok"]["url"], "https://api.acme.test/v1/decisions");

    let res = json!({ "status": 200, "headers": [], "body": json!({ "answers": { "q": { "noul": 0.5 } } }).to_string() });
    let parsed: Value =
        serde_json::from_str(&json::parse_dialect(&resolved, &request, &res.to_string())).unwrap();
    assert_eq!(parsed["ok"]["answers"]["q"]["probability"], 0.5);

    // The response check applies: a probability out of range is BadResponse.
    let bad = json!({ "status": 200, "headers": [], "body": json!({ "answers": { "q": { "noul": 1.5 } } }).to_string() });
    let parsed: Value =
        serde_json::from_str(&json::parse_dialect(&resolved, &request, &bad.to_string())).unwrap();
    assert_eq!(parsed["error"]["code"], "BadResponse");

    let gemini: Value = serde_json::from_str(&json::dialect("gemini")).unwrap();
    assert_eq!(gemini["ok"]["auth"]["header"], "x-goog-api-key");
    let none: Value = serde_json::from_str(&json::dialect("nope")).unwrap();
    assert_eq!(none["error"]["details"]["reason"], "unknown_provider");
}

/// The examples in docs/dialects.md must stay valid.
#[test]
fn documented_examples_resolve() {
    let doc = std::fs::read_to_string("docs/dialects.md").unwrap();
    let block = doc
        .split("```jsonc")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let mut n = 0;
    // Each example is a JSON object after its comment lines.
    let json: String = block
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for v in serde_json::Deserializer::from_str(&json).into_iter::<Value>() {
        let v = v.unwrap();
        Dialect::from_value(v.clone()).unwrap_or_else(|e| panic!("{v}: {e}"));
        n += 1;
    }
    assert_eq!(n, 3);
}

#[test]
fn auth_scheme_is_inferred_and_required_is_inherited() {
    let from = |base: &str, auth: Value| {
        Dialect::from_value(json!({ "extends": base, "base_url": "https://h.test", "auth": auth }))
            .unwrap()
    };
    let c = cfg(json!({ "api_key": "k" }));
    let auth = |d: &Dialect, h: &str| {
        let http = d.build("m", &yesno(), &c).unwrap();
        header(&http, h).map(str::to_owned)
    };
    // Scheme inferred from the header name.
    let bearer = from("systemone", json!({}));
    let raw = from("systemone", json!({ "header": "x-api-key" }));
    assert_eq!(auth(&bearer, "authorization").as_deref(), Some("Bearer k"));
    assert_eq!(auth(&raw, "x-api-key").as_deref(), Some("k"));
    // An explicit scheme still wins.
    let forced = from(
        "systemone",
        json!({ "header": "x-token", "scheme": "bearer" }),
    );
    assert_eq!(auth(&forced, "x-token").as_deref(), Some("Bearer k"));
    // `required` comes from the base (objects merge) unless set.
    assert!(raw.build("m", &yesno(), &Config::default()).is_ok());
    let strict = from(
        "systemone",
        json!({ "header": "x-api-key", "required": true }),
    );
    let e = strict.build("m", &yesno(), &Config::default()).unwrap_err();
    assert_eq!(reason(&e), Reason::MissingConfig);
    let gemini_like = from("gemini", json!({ "header": "x-api-key" }));
    assert!(gemini_like
        .build("m", &yesno(), &Config::default())
        .is_err());
    // A dialect's own `Auth` with nothing set requires a key.
    assert!(Auth::default().required);
}

#[test]
fn options_placeholders_fill_the_path() {
    let d = Dialect::from_value(json!({
        "extends": "systemone",
        "base_url": "https://api.acme.test",
        "path": "/{options.region}/v1/{model}"
    }))
    .unwrap();
    let with = |region: &str| cfg(json!({ "options": { "region": region } }));
    let http = d.build("m1", &yesno(), &with("eu-west")).unwrap();
    assert_eq!(http.url, "https://api.acme.test/eu-west/v1/m1");
    for c in [Config::default(), with("../x")] {
        let e = d.build("m1", &yesno(), &c).unwrap_err();
        assert_eq!(e.details.unwrap().field.as_deref(), Some("options.region"));
    }
}
