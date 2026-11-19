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

use crate::provider::{http_error, post};
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

    /// Documented per-request caps.
    fn limits(&self) -> Limits {
        const NONE: usize = usize::MAX;
        match self.preset {
            Preset::Jev => Limits {
                questions: NONE,
                options: NONE,
                levels: 10,
            },
            Preset::Clef => Limits {
                questions: 64,
                options: NONE,
                levels: 10,
            },
            Preset::Gemini => Limits {
                questions: 1000,
                options: 255,
                levels: 255,
            },
            Preset::Custom => Limits {
                questions: NONE,
                options: NONE,
                levels: NONE,
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
        let unsupported = |m: &str| Err(Error::new(ErrorCode::Unsupported, m));
        let media = &req.media;
        if self.preset == Preset::Gemini
            && media
                .iter()
                .any(|m| m.is_image() && !GEMINI_IMAGE_MIME.contains(&m.mime_type.as_str()))
        {
            return unsupported("Gemini does not accept this image MIME type");
        }
        body.state = Some(Cow::Borrowed(&req.input));
        if self.preset != Preset::Gemini {
            req.require_images_only()?;
        }
        match self.preset {
            // Typed blocks; text-only input stays a plain string. JSON goes as text.
            Preset::Gemini => {
                body.state = None;
                let text = match &req.input {
                    Value::String(_) => req.input.clone(),
                    other => Value::String(other.to_string()),
                };
                body.input = Some(if media.is_empty() {
                    text
                } else {
                    let mut blocks = vec![json!({ "type": "text", "text": text })];
                    for m in media {
                        let Some(kind) = m.kind() else {
                            return unsupported(&format!("unsupported media `{}`", m.mime_type));
                        };
                        blocks.push(
                            json!({ "type": kind, "data": m.data, "mime_type": m.mime_type }),
                        );
                    }
                    Value::Array(blocks)
                });
            }
            Preset::Clef => {
                if media.len() > 4 {
                    return unsupported("Clef takes at most 4 images");
                }
                if media.iter().any(|i| {
                    !matches!(
                        i.mime_type.as_str(),
                        "image/png" | "image/jpeg" | "image/webp"
                    )
                }) {
                    return unsupported("Clef images must be PNG, JPEG or WebP");
                }
                body.images = media.iter().map(Media::data_url).collect();
            }
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
            Preset::Custom if !media.is_empty() => {
                return unsupported("images are not supported for custom servers")
            }
            _ => {}
        }
        Ok(())
    }
}

struct Limits {
    questions: usize,
    options: usize,
    levels: usize,
}

/// Image MIME types Gemini Decisions accepts (decisions-api, "Input").
const GEMINI_IMAGE_MIME: [&str; 8] = [
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/heic",
    "image/heif",
    "image/gif",
    "image/bmp",
    "image/tiff",
];

/// `error.code` values that mean the input was blocked (decisions-api, "Request").
const GEMINI_BLOCK_CODES: [&str; 5] = [
    "safety",
    "prohibited_content",
    "blocklist",
    "image_safety",
    "content_blocked",
];

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
#[serde(tag = "type", rename_all = "lowercase")]
enum Wire<'a> {
    Noul {
        instructions: &'a str,
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
    fn build(
        &self,
        model: &str,
        req: &DecisionRequest,
        cfg: &Config,
    ) -> Result<HttpRequest, Error> {
        req.validate()?;
        let limits = self.limits();
        let unsupported = |m: String| Err(Error::new(ErrorCode::Unsupported, m));
        if req.questions.len() > limits.questions {
            return unsupported(format!("max {} questions per request", limits.questions));
        }
        for (id, q) in &req.questions {
            match q {
                Question::Score { levels, .. } if levels.len() > limits.levels => {
                    return unsupported(format!(
                        "question `{id}`: max {} score levels",
                        limits.levels
                    ));
                }
                Question::Choice { options, .. } if options.len() > limits.options => {
                    return unsupported(format!("question `{id}`: max {} options", limits.options));
                }
                _ => {}
            }
        }
        let questions = req
            .questions
            .iter()
            .map(|(id, q)| {
                let wire = match q {
                    Question::YesNo { instructions } => Wire::Noul { instructions },
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
        let body = serde_json::to_vec(&body)
            .map_err(|e| Error::new(ErrorCode::InvalidRequest, e.to_string()))?;
        let url = self.url(model, cfg)?;
        let mut http = match self.preset {
            Preset::Gemini => post(url, "x-goog-api-key", false, cfg, body),
            _ => post(url, "authorization", true, cfg, body),
        };
        http.headers.extend(extra_header);
        Ok(http)
    }

    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error> {
        if self.preset == Preset::Gemini && res.status == 400 && blocked_by_error(&res.body) {
            return Ok(refused(req, None, None));
        }
        if !(200..300).contains(&res.status) {
            return Err(http_error(res));
        }
        let bad = |m: String| Error {
            status: Some(res.status),
            ..Error::new(ErrorCode::BadResponse, m)
        };
        let mut reply: Reply = serde_json::from_slice(&res.body).map_err(|e| bad(e.to_string()))?;
        if let Some(inner) = reply.result.take() {
            reply = *inner;
        }
        let input_tokens = reply.usage.and_then(|u| u.input_tokens);

        // Input blocked by a safety filter: nothing was evaluated.
        if reply.prompt_feedback.and_then(|f| f.block_reason).is_some() {
            return Ok(refused(req, reply.model, input_tokens));
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
                Question::Choice { .. } => Answer::Choice {
                    choice: raw.choice.ok_or_else(|| missing("choice"))?,
                    confidence: raw.confidence,
                    probabilities: raw.probabilities,
                },
                Question::Score { levels, .. } => Answer::Score {
                    value: raw.score.ok_or_else(|| missing("score"))?,
                    confidence: raw.confidence,
                    // Providers key probabilities by 0-based level index.
                    probabilities: levels
                        .iter()
                        .enumerate()
                        .filter_map(|(i, l)| {
                            Some((l.id.clone(), *raw.probabilities.get(&i.to_string())?))
                        })
                        .collect(),
                },
            };
            answers.insert(id.clone(), answer);
        }
        Ok(DecisionResponse {
            model: reply.model,
            answers,
            input_tokens,
        })
    }
}

/// Every question refused: the input was blocked, nothing was evaluated.
fn refused(
    req: &DecisionRequest,
    model: Option<String>,
    input_tokens: Option<u32>,
) -> DecisionResponse {
    DecisionResponse {
        model,
        answers: req
            .questions
            .keys()
            .map(|id| (id.clone(), Answer::Refused))
            .collect(),
        input_tokens,
    }
}

/// Gemini's documented HTTP 400 safety shape: `{"error": {"code": "safety", ..}}`.
fn blocked_by_error(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["code"]
                .as_str()
                .map(|c| GEMINI_BLOCK_CODES.contains(&c))
        })
        .unwrap_or(false)
}
