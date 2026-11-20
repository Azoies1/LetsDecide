//! Adapter for the System One wire family: Jev, Clef, Gemini and compatible
//! servers. One shape, four presets.
//!
//! Verified against: Jev's OpenAPI spec (api.typesafe.ai/openapi.json), Clef's
//! Cloudflare docs, and Google's official Gemini Decisions pages (read
//! 2026-11-19):
//! - https://aistudio.google.com/api-reference/decisions/create-decision
//! - https://aistudio.google.com/docs/decisions-api
//!
//! Checked live (2026-11-19): Gemini replies carry no top-level `model` (so
//! `DecisionResponse.model` is `None`; it is still parsed if present). Still
//! unverified: which of the two documented safety-block shapes (200 +
//! `prompt_feedback.block_reason`, or HTTP 400 + `error.code`) applies when.
//! Both are handled.

use crate::error::safe_serde_message;
use crate::gemini;
use crate::model::{confidence, DOCUMENT_MIME};
use crate::provider::{encode, post};
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Presets: `jev`, `clef`, `gemini`, `systemone` (custom base URL).
pub struct SystemOne {
    preset: Preset,
}

#[derive(Clone, Copy, PartialEq)]
enum Preset {
    Jev,
    Clef,
    Gemini,
    Custom,
}

impl SystemOne {
    pub fn preset(name: &str) -> Option<Self> {
        let preset = match name {
            "jev" => Preset::Jev,
            "clef" => Preset::Clef,
            "gemini" => Preset::Gemini,
            "systemone" => Preset::Custom,
            _ => return None,
        };
        Some(Self { preset })
    }

    fn url(&self, model: &str, cfg: &Config) -> Result<String, Error> {
        let missing = |what| Error::new(ErrorCode::InvalidRequest, format!("{what} is required"));
        let base = |default: &str| {
            cfg.base_url
                .as_deref()
                .unwrap_or(default)
                .trim_end_matches('/')
                .to_owned()
        };
        Ok(match self.preset {
            Preset::Jev => base("https://api.typesafe.ai") + "/v1/systemone",
            Preset::Gemini => {
                base("https://generativelanguage.googleapis.com") + "/v1beta/decisions"
            }
            Preset::Custom => {
                if cfg.base_url.is_none() {
                    return Err(missing("base_url"));
                }
                base("") + "/v1/systemone"
            }
            Preset::Clef => {
                let id = cfg
                    .account_id
                    .as_deref()
                    .ok_or_else(|| missing("account_id"))?;
                let root = base("https://api.cloudflare.com");
                format!("{root}/client/v4/accounts/{id}/ai/run/@cf/cloudflare/{model}")
            }
        })
    }

    /// Gemini needs `api_key`; Clef needs `api_key` or an OAuth `access_token`.
    /// Fails before any request is built instead of surfacing a late 401.
    fn require_credentials(&self, cfg: &Config) -> Result<(), Error> {
        let need = |m: &str| Err(Error::new(ErrorCode::InvalidRequest, m));
        match self.preset {
            Preset::Gemini if cfg.api_key.is_none() => need("api_key is required for gemini"),
            Preset::Clef if cfg.api_key.is_none() && cfg.access_token.is_none() => {
                need("api_key or access_token is required for clef")
            }
            _ => Ok(()),
        }
    }

    /// Documented per-request caps and accepted media.
    fn caps(&self) -> Capabilities {
        let mime = |l: &[&str]| l.iter().map(|m| (*m).to_owned()).collect();
        match self.preset {
            Preset::Jev => Capabilities {
                max_levels: Some(10),
                media: mime(&["image/png", "image/jpeg"]),
                ..Default::default()
            },
            Preset::Clef => Capabilities {
                max_questions: Some(64),
                max_levels: Some(10),
                max_media: Some(4),
                media: mime(&["image/png", "image/jpeg", "image/webp"]),
                ..Default::default()
            },
            Preset::Gemini => {
                let mut media: Vec<String> = mime(&gemini::IMAGE_MIME);
                media.extend(mime(&["audio/*", "video/*"]));
                media.extend(mime(&DOCUMENT_MIME));
                Capabilities {
                    max_questions: Some(1000),
                    max_options: Some(255),
                    max_levels: Some(255),
                    media,
                    wide_ids: true,
                    ..Default::default()
                }
            }
            Preset::Custom => Capabilities {
                locality: Locality::Network,
                ..Default::default()
            },
        }
    }

    /// Puts input and media where this preset expects them.
    fn attach_input<'a>(
        &self,
        req: &'a DecisionRequest,
        body: &mut Body<'a>,
        header: &mut Option<(String, String)>,
    ) -> Result<(), Error> {
        // Media types and counts were checked against `caps()` already.
        let unsupported = |m: &str| Err(Error::new(ErrorCode::Unsupported, m));
        let media = &req.media;
        body.state = Some(Cow::Borrowed(&req.input));
        match self.preset {
            Preset::Gemini => {
                body.state = None;
                body.input = Some(gemini::input(req)?);
            }
            Preset::Clef => body.images = media.iter().map(Media::data_url).collect(),
            Preset::Jev if !media.is_empty() => {
                // Placeholders in `state` are swapped for the assets server-side.
                let mut refs = vec![];
                for (n, img) in media.iter().enumerate() {
                    let format = match img.mime_type.as_str() {
                        "image/png" => "png",
                        "image/jpeg" => "jpeg",
                        _ => return unsupported("Jev images must be PNG or JPEG"),
                    };
                    let key = format!("letsdecide-image-{n}");
                    let asset =
                        json!({ "type": "image", "format": format, "data_base64": img.data });
                    body.assets.insert(key.clone(), asset);
                    refs.push(key);
                }
                body.state = Some(Cow::Owned(match &req.input {
                    Value::String(t) => Value::String(format!("{t}\n{}", refs.join("\n"))),
                    other => Value::Array(
                        std::iter::once(other.clone())
                            .chain(refs.into_iter().map(Value::String))
                            .collect(),
                    ),
                }));
                // Jev gates image requests behind this header.
                *header = Some(("typesafe-danger-zone-images".into(), "true".into()));
            }
            Preset::Jev | Preset::Custom => {}
        }
        Ok(())
    }
}

// Request wire types, borrowed so the body is serialised straight to one buffer.
#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    // Jev and Clef call the input `state`; Gemini calls it `input`.
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<Cow<'a, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<Value>,
    // Clef: data URLs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
    // Jev: placeholder → image, referenced from `state`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    assets: BTreeMap<String, Value>,
    questions: BTreeMap<&'a str, Wire<'a>>,
}

#[derive(Serialize)]
struct NoulCriteria<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    true_description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    false_description: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Wire<'a> {
    Noul {
        instructions: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria<'a>>,
    },
    Choice {
        instructions: &'a str,
        criteria: &'a BTreeMap<Id, String>,
    },
    Score {
        instructions: &'a str,
        criteria: Vec<&'a str>,
    },
}

// Response wire types.
#[derive(Deserialize)]
struct Reply {
    /// Cloudflare (Clef) wraps the payload: `{"result": {...}, "success": true}`.
    result: Option<Box<Reply>>,
    model: Option<String>,
    #[serde(default)]
    answers: BTreeMap<Id, RawAnswer>,
    usage: Option<Usage>,
    prompt_feedback: Option<PromptFeedback>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(alias = "total_input_tokens")]
    input_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct PromptFeedback {
    block_reason: Option<String>,
    block_reason_message: Option<String>,
}

#[derive(Deserialize)]
struct RawAnswer {
    noul: Option<f32>,
    choice: Option<Id>,
    score: Option<f32>,
    confidence: Option<f32>,
    #[serde(default)]
    probabilities: BTreeMap<String, f32>,
}

impl Provider for SystemOne {
    fn capabilities(&self, _model: &str) -> Capabilities {
        self.caps()
    }

    fn build(
        &self,
        model: &str,
        req: &DecisionRequest,
        cfg: &Config,
    ) -> Result<HttpRequest, Error> {
        self.caps().check(req)?;
        if self.preset != Preset::Gemini {
            req.require_portable(false)?;
        }
        let questions = req
            .questions
            .iter()
            .map(|(id, q)| {
                let wire = match q {
                    Question::YesNo {
                        instructions,
                        yes_description,
                        no_description,
                    } => Wire::Noul {
                        instructions,
                        criteria: (yes_description.is_some() || no_description.is_some())
                            .then_some(NoulCriteria {
                                true_description: yes_description.as_deref(),
                                false_description: no_description.as_deref(),
                            }),
                    },
                    Question::Choice {
                        instructions,
                        options,
                    } => Wire::Choice {
                        instructions,
                        criteria: options,
                    },
                    Question::Score {
                        instructions,
                        levels,
                    } => Wire::Score {
                        instructions,
                        criteria: levels.iter().map(|l| l.description.as_str()).collect(),
                    },
                };
                (id.as_str(), wire)
            })
            .collect();
        let mut body = Body {
            model,
            state: None,
            input: None,
            images: vec![],
            assets: BTreeMap::new(),
            questions,
        };
        let mut extra_header = None;
        self.attach_input(req, &mut body, &mut extra_header)?;
        let body = encode(&body, req)?;
        let url = self.url(model, cfg)?;
        self.require_credentials(cfg)?;
        let mut http = match self.preset {
            Preset::Gemini => post(url, "x-goog-api-key", false, cfg, body),
            _ => post(url, "authorization", true, cfg, body),
        };
        http.headers.extend(extra_header);
        apply_headers(&mut http, cfg)?;
        Ok(http)
    }

    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error> {
        if self.preset == Preset::Gemini && res.status == 400 {
            if let Some(reason) = gemini::blocked_by_error(&res.body) {
                return Ok(refused(req, None, None, Some(reason)));
            }
        }
        if !(200..300).contains(&res.status) {
            return Err(http_error(res));
        }
        let bad = |m: String| Error {
            status: Some(res.status),
            ..Error::new(ErrorCode::BadResponse, m)
        };
        let mut reply: Reply = serde_json::from_slice(&res.body)
            .map_err(|e| bad(safe_serde_message("response body", &e)))?;
        if let Some(inner) = reply.result.take() {
            reply = *inner;
        }
        let input_tokens = reply.usage.take().and_then(|u| u.input_tokens);

        // Input blocked by a safety filter: nothing was evaluated.
        if let Some(f) = reply.prompt_feedback.take() {
            if let Some(reason) = f.block_reason {
                let reason = f.block_reason_message.unwrap_or(reason);
                return Ok(refused(req, reply.model, input_tokens, Some(reason)));
            }
        }

        let mut answers = BTreeMap::new();
        for (id, q) in &req.questions {
            let raw = reply
                .answers
                .remove(id)
                .ok_or_else(|| bad(format!("missing answer `{id}`")))?;
            let missing = |f: &str| bad(format!("answer `{id}` missing `{f}`"));
            let answer = match q {
                Question::YesNo { .. } => Answer::YesNo {
                    probability: raw.noul.ok_or_else(|| missing("noul"))?,
                },
                Question::Choice { .. } => {
                    let choice = raw.choice.ok_or_else(|| missing("choice"))?;
                    let (confidence, confidence_source) =
                        confidence(raw.confidence, raw.probabilities.get(&choice).copied());
                    Answer::Choice {
                        choice,
                        confidence,
                        confidence_source,
                        probabilities: raw.probabilities,
                    }
                }
                Question::Score { levels, .. } => {
                    // Providers key probabilities by 0-based level index.
                    let probabilities: BTreeMap<Id, f32> = levels
                        .iter()
                        .enumerate()
                        .filter_map(|(i, l)| {
                            Some((l.id.clone(), *raw.probabilities.get(&i.to_string())?))
                        })
                        .collect();
                    let top = probabilities.values().copied().reduce(f32::max);
                    let (confidence, confidence_source) = confidence(raw.confidence, top);
                    Answer::Score {
                        value: raw.score.ok_or_else(|| missing("score"))?,
                        confidence,
                        confidence_source,
                        probabilities,
                    }
                }
            };
            answers.insert(id.clone(), answer);
        }
        Ok(DecisionResponse {
            model: reply.model,
            answers,
            input_tokens,
            probability_source: ProbabilitySource::Provider,
            block_reason: None,
        })
    }
}

/// Every question refused: the input was blocked, nothing was evaluated.
fn refused(
    req: &DecisionRequest,
    model: Option<String>,
    input_tokens: Option<u32>,
    block_reason: Option<String>,
) -> DecisionResponse {
    DecisionResponse {
        model,
        answers: req
            .questions
            .keys()
            .map(|id| (id.clone(), Answer::Refused))
            .collect(),
        input_tokens,
        probability_source: ProbabilitySource::Provider,
        block_reason,
    }
}
