//! Dialects: a provider described as data.
//!
//! Every built-in is a dialect (`spec/dialects/*.json`), and so is anything a
//! user registers that speaks one of the two wire families with small
//! differences: another path, auth header, input field, response envelope,
//! media encoding or limits. A dialect can `extends` a built-in and override
//! fields, so a vendor's API change is a data edit, not a release.
//!
//! Deliberately not a template language. The parts that are real logic
//! (question mapping, media encodings, refusal handling) are tested code in
//! the core, picked by name. An API that needs more is a codec (see
//! `docs/custom-providers.md`), or a new strategy here once a second real
//! provider needs it.

use crate::error::{Details, Reason};
use crate::{
    openai, systemone, Capabilities, Config, DecisionRequest, DecisionResponse, Error, ErrorCode,
    HttpRequest, HttpResponse, Provider,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The two request/response shapes every known decision API uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// `questions` map with `noul` / `choice` / `score` (Jev, Clef, Gemini, ...).
    Systemone,
    /// Ordered `questions` array with `predicate` / `choice` / `score` (OpenAI).
    Openai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    /// `Bearer <access_token or api_key>`.
    Bearer,
    /// The `api_key` as-is.
    Raw,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Auth {
    /// Header the credential goes in, e.g. `authorization`, `x-api-key`.
    #[serde(default = "authorization")]
    pub header: String,
    /// Inferred when unset: `bearer` for `authorization`, else `raw`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheme: Option<AuthScheme>,
    /// Fail before sending when no credential is set, instead of a late 401.
    #[serde(default = "yes")]
    pub required: bool,
}

fn authorization() -> String {
    "authorization".into()
}

fn yes() -> bool {
    true
}

impl Default for Auth {
    fn default() -> Self {
        Self {
            header: authorization(),
            scheme: None,
            required: true,
        }
    }
}

impl Auth {
    /// The explicit scheme, else `bearer` for `authorization` and `raw` for
    /// any other header (`x-api-key`, `api-key`, `x-goog-api-key`, ...).
    pub fn scheme(&self) -> AuthScheme {
        self.scheme
            .unwrap_or(if self.header.eq_ignore_ascii_case("authorization") {
                AuthScheme::Bearer
            } else {
                AuthScheme::Raw
            })
    }
}

/// Where a System One body carries the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputField {
    /// `state`, JSON input as-is (Jev, Clef).
    #[default]
    State,
    /// `input`, JSON input serialised to text (Gemini).
    Input,
}

/// How media is attached. Each is tested code in the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaEncoding {
    /// No media.
    #[default]
    None,
    /// Jev: `assets` map, placeholders appended to the input, and Jev's
    /// image gate header.
    JevAssets,
    /// Clef: `images` array of data URLs.
    DataUrlList,
    /// Gemini: typed content blocks (`image`, `audio`, `video`, `document`),
    /// inline or by URI, with per-media options.
    TypedBlocks,
    /// OpenAI: `input_image` content parts (data URLs or public image URLs).
    ContentParts,
}

/// A provider described as data. See the module docs.
///
/// Users write `extends` plus the fields that differ; `family` comes from
/// the built-in they extend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dialect {
    pub family: Family,
    /// Default endpoint; `Config.base_url` overrides it. `None`: the caller
    /// must set `base_url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Path after the base URL. Placeholders: `{model}`, `{account_id}`,
    /// `{options.<name>}`.
    pub path: String,
    #[serde(default)]
    pub auth: Auth,
    /// System One only.
    #[serde(default)]
    pub input_field: InputField,
    #[serde(default)]
    pub media_encoding: MediaEncoding,
    /// Replies wrapped in an object under this key (Clef: `result`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<String>,
    /// HTTP 400 `error.code` values meaning "input blocked": every answer
    /// becomes `Refused` instead of an error (Gemini).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_error_codes: Vec<String>,
    #[serde(default)]
    pub capabilities: Capabilities,
}

/// Jev gates image requests behind this header (part of `jev_assets`).
pub(crate) const JEV_IMAGE_HEADER: (&str, &str) = ("typesafe-danger-zone-images", "true");

const BUILTIN_SOURCES: [(&str, &str); 5] = [
    ("jev", include_str!("../spec/dialects/jev.json")),
    ("clef", include_str!("../spec/dialects/clef.json")),
    ("gemini", include_str!("../spec/dialects/gemini.json")),
    ("openai", include_str!("../spec/dialects/openai.json")),
    ("systemone", include_str!("../spec/dialects/systemone.json")),
];

fn builtins() -> &'static BTreeMap<&'static str, Value> {
    static B: OnceLock<BTreeMap<&'static str, Value>> = OnceLock::new();
    B.get_or_init(|| {
        BUILTIN_SOURCES
            .iter()
            .map(|(n, src)| {
                let v: Value = serde_json::from_str(src).expect("built-in dialect is JSON");
                (*n, strip_comments(v))
            })
            .collect()
    })
}

/// Keys starting with `_` are comments (`_doc`).
fn strip_comments(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .map(|(k, v)| (k, strip_comments(v)))
                .collect(),
        ),
        other => other,
    }
}

/// Objects merge key by key; anything else (arrays included) is replaced.
fn merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (slot, v) => *slot = v,
    }
}

fn invalid(message: impl Into<String>, field: &str) -> Error {
    Error::detailed(
        ErrorCode::InvalidRequest,
        message,
        Details::new(Reason::InvalidConfig).field(field),
    )
}

impl Dialect {
    /// A built-in dialect: `jev`, `clef`, `gemini`, `openai`, `systemone`.
    pub fn builtin(name: &str) -> Option<Self> {
        let v = builtins().get(name)?.clone();
        Some(serde_json::from_value(v).expect("built-in dialect is valid"))
    }

    /// A built-in as JSON, to read or copy as a starting point.
    pub fn builtin_json(name: &str) -> Option<Value> {
        builtins().get(name).cloned()
    }

    /// Parses and validates a dialect: `{"extends": "<built-in>", ...}`
    /// starts from that built-in and merges the rest over it (objects key by
    /// key, arrays replaced). `extends` is required (`systemone` or `openai`
    /// for a fresh one) and `family` comes from it. Unknown fields are an
    /// error, so typos fail.
    pub fn from_value(v: Value) -> Result<Self, Error> {
        let Value::Object(mut obj) = strip_comments(v) else {
            return Err(invalid("a dialect is a JSON object", "dialect"));
        };
        let mut base = match obj.remove("extends") {
            None => {
                return Err(invalid(
                    "a dialect `extends` a built-in: jev, clef, gemini, openai or systemone",
                    "extends",
                ))
            }
            Some(Value::String(name)) => {
                builtins().get(name.as_str()).cloned().ok_or_else(|| {
                    invalid(
                        format!("`extends`: no built-in dialect `{name}`"),
                        "extends",
                    )
                })?
            }
            Some(_) => return Err(invalid("`extends` must be a built-in name", "extends")),
        };
        // `family` comes from the base; restating it is fine, changing it isn't.
        if let Some(f) = obj.get("family") {
            if Some(f) != base.get("family") {
                return Err(invalid(
                    "`family` comes from `extends`; extend `systemone` or `openai`",
                    "family",
                ));
            }
        }
        merge(&mut base, Value::Object(obj));
        let d: Dialect = serde_json::from_value(base)
            .map_err(|e| invalid(crate::error::safe_dialect_message(&e), "dialect"))?;
        d.validate()?;
        Ok(d)
    }

    /// A full dialect as `resolve_dialect` returns it (no `extends`).
    pub(crate) fn from_resolved(json: &str) -> Result<Self, Error> {
        let d: Dialect = serde_json::from_str(json)
            .map_err(|e| invalid(crate::error::safe_dialect_message(&e), "dialect"))?;
        d.validate()?;
        Ok(d)
    }

    /// As [`from_value`](Self::from_value), from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, Error> {
        let v: Value = serde_json::from_str(json)
            .map_err(|e| invalid(crate::error::safe_serde_message("dialect", &e), "dialect"))?;
        Self::from_value(v)
    }

    /// Checks the combination makes sense, so a bad dialect fails when it is
    /// registered rather than on the first request.
    pub fn validate(&self) -> Result<(), Error> {
        if !self.path.starts_with('/') {
            return Err(invalid("`path` must start with `/`", "path"));
        }
        let mut rest = self.path.as_str();
        while let Some(i) = rest.find('{') {
            let end = rest[i..]
                .find('}')
                .ok_or_else(|| invalid("`path` has an unclosed `{`", "path"))?;
            let name = &rest[i + 1..i + end];
            let option_ok = name.strip_prefix("options.").is_some_and(|o| {
                !o.is_empty()
                    && o.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            });
            if !(matches!(name, "model" | "account_id") || option_ok) {
                return Err(invalid(
                    "`path` placeholders are `{model}`, `{account_id}` and `{options.<name>}`",
                    "path",
                ));
            }
            rest = &rest[i + end + 1..];
        }
        if let Some(b) = &self.base_url {
            check_base_url(b)?;
        }
        if !is_token(&self.auth.header) {
            return Err(invalid("`auth.header` is not a valid header name", "auth"));
        }
        let ok_media = match self.family {
            Family::Openai => matches!(
                self.media_encoding,
                MediaEncoding::None | MediaEncoding::ContentParts
            ),
            Family::Systemone => self.media_encoding != MediaEncoding::ContentParts,
        };
        if !ok_media {
            return Err(invalid(
                "`media_encoding` doesn't fit this `family`",
                "media_encoding",
            ));
        }
        if self.media_encoding == MediaEncoding::None && !self.capabilities.media.is_empty() {
            return Err(invalid(
                "`capabilities.media` needs a `media_encoding`",
                "media_encoding",
            ));
        }
        Ok(())
    }

    /// `base_url` (config, else the dialect's) + `path`, placeholders filled.
    pub(crate) fn url(&self, model: &str, cfg: &Config) -> Result<String, Error> {
        let missing = |field: &str| {
            Error::detailed(
                ErrorCode::InvalidRequest,
                format!("{field} is required"),
                Details::new(Reason::MissingConfig).field(field),
            )
        };
        let base = match (&cfg.base_url, &self.base_url) {
            (Some(b), _) => {
                check_base_url(b)?;
                b
            }
            (None, Some(b)) => b,
            (None, None) => return Err(missing("base_url")),
        };
        let mut path = self.path.clone();
        if path.contains("{model}") {
            path = path.replace("{model}", path_segment(model, "model")?);
        }
        if path.contains("{account_id}") {
            let id = cfg
                .account_id
                .as_deref()
                .ok_or_else(|| missing("account_id"))?;
            path = path.replace("{account_id}", path_segment(id, "account_id")?);
        }
        while let Some(i) = path.find("{options.") {
            let end = i + path[i..].find('}').expect("validated");
            let name = path[i + "{options.".len()..end].to_owned();
            let field = format!("options.{name}");
            let value = match cfg.options.get(&name) {
                Some(Value::String(v)) => v.clone(),
                Some(Value::Number(n)) => n.to_string(),
                Some(_) => return Err(invalid(format!("`{field}` must be a string"), &field)),
                None => return Err(missing(&field)),
            };
            path.replace_range(i..=end, path_segment(&value, &field)?);
        }
        Ok(format!("{}{path}", base.trim_end_matches('/')))
    }

    /// The credential header, or an error when one is required and missing.
    pub(crate) fn credential(&self, cfg: &Config) -> Result<Option<(String, String)>, Error> {
        let value = match self.auth.scheme() {
            AuthScheme::Bearer => cfg
                .access_token
                .as_ref()
                .or(cfg.api_key.as_ref())
                .map(|t| format!("Bearer {t}")),
            AuthScheme::Raw => cfg.api_key.clone(),
        };
        match value {
            Some(v) => Ok(Some((self.auth.header.clone(), v))),
            None if self.auth.required => {
                let what = match self.auth.scheme() {
                    AuthScheme::Bearer => "api_key or access_token",
                    AuthScheme::Raw => "api_key",
                };
                Err(Error::detailed(
                    ErrorCode::InvalidRequest,
                    format!("{what} is required"),
                    Details::new(Reason::MissingConfig).field("api_key"),
                ))
            }
            None => Ok(None),
        }
    }

    /// The reply body with the `envelope` unwrapped, if there is one.
    pub(crate) fn unwrap_envelope(&self, mut v: Value) -> Value {
        if let Some(key) = &self.envelope {
            if let Some(inner) = v.get_mut(key).map(Value::take) {
                if inner.is_object() {
                    return inner;
                }
            }
        }
        v
    }

    /// For a 400: the block reason when `error.code` is a listed block code.
    pub(crate) fn blocked(&self, res: &HttpResponse) -> Option<String> {
        if res.status != 400 || self.blocked_error_codes.is_empty() {
            return None;
        }
        let v = serde_json::from_slice::<Value>(&res.body).ok()?;
        let code = v["error"]["code"].as_str()?;
        self.blocked_error_codes
            .iter()
            .any(|c| c == code)
            .then(|| v["error"]["message"].as_str().unwrap_or(code).to_owned())
    }
}

fn check_base_url(b: &str) -> Result<(), Error> {
    if b.starts_with("https://") || b.starts_with("http://") {
        Ok(())
    } else {
        Err(invalid(
            "`base_url` must start with https:// or http://",
            "base_url",
        ))
    }
}

/// Values put into the path must stay one segment: no `/`, `?`, `#`, `%`,
/// `..`, spaces or control characters.
fn path_segment<'a>(v: &'a str, field: &str) -> Result<&'a str, Error> {
    let bad = v.is_empty()
        || v == "."
        || v == ".."
        || v.bytes()
            .any(|b| matches!(b, b'/' | b'?' | b'#' | b'%' | b'\\') || b <= b' ' || b == 0x7f);
    if bad {
        Err(invalid(
            format!("`{field}` can't be used in a URL path"),
            field,
        ))
    } else {
        Ok(v)
    }
}

fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

impl Provider for Dialect {
    fn capabilities(&self, _model: &str) -> Capabilities {
        self.capabilities.clone()
    }

    fn build(
        &self,
        model: &str,
        req: &DecisionRequest,
        cfg: &Config,
    ) -> Result<HttpRequest, Error> {
        match self.family {
            Family::Systemone => systemone::build(self, model, req, cfg),
            Family::Openai => openai::build(self, model, req, cfg),
        }
    }

    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error> {
        match self.family {
            Family::Systemone => systemone::parse(self, req, res),
            Family::Openai => openai::parse(self, req, res),
        }
    }
}
