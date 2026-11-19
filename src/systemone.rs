//! Adapter for the System One wire family: Jev, Clef, Gemini and compatible
//! servers. One shape, four presets.
//!
//! Verified against: Jev's OpenAPI spec (api.typesafe.ai/openapi.json), Clef's
//! Cloudflare docs, and Google's official Gemini Decisions pages (read
//! 2026-11-19):
//! - https://aistudio.google.com/api-reference/decisions/create-decision
//! - https://aistudio.google.com/docs/decisions-api
//!
//! Still unverified for Gemini: whether replies carry a top-level `model`
//! (parsed if present), and which of the two documented safety-block shapes
//! (200 + `prompt_feedback.block_reason`, or HTTP 400 + `error.code`) applies
//! when. Both are handled.

use crate::model::IdRule;
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
        if self.preset == Preset::Gemini {
            check_gemini_sizes(media)?;
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
                        let mut block = serde_json::Map::new();
                        for (k, v) in &m.options {
                            if !gemini_option_allowed(kind, k) {
                                return unsupported(&format!(
                                    "`{k}` is not a Gemini {kind} option"
                                ));
                            }
                            block.insert(k.clone(), v.clone());
                        }
                        block.insert("type".into(), kind.into());
                        match &m.uri {
                            Some(u) => {
                                check_gemini_uri(u)?;
                                block.insert("uri".into(), u.clone().into());
                            }
                            None => {
                                block.insert("data".into(), m.data.clone().into());
                            }
                        }
                        block.insert("mime_type".into(), m.mime_type.clone().into());
                        blocks.push(Value::Object(block));
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

/// Inline-data caps (decisions-api, "Input"): 100 MB per request, 50 MB for PDF.
const GEMINI_MAX_INLINE: usize = 100 * 1024 * 1024;
const GEMINI_MAX_PDF: usize = 50 * 1024 * 1024;

/// Option names per block type, from the create-decision schema.
fn gemini_option_allowed(kind: &str, key: &str) -> bool {
    let allowed: &[&str] = match kind {
        "image" => &["resolution"],
        "audio" => &["channels", "sample_rate", "processing"],
        "video" => &[
            "resolution",
            "fps",
            "start_offset",
            "end_offset",
            "processing",
            "name",
        ],
        "document" => &["page_options", "spreadsheet_options"],
        _ => &[],
    };
    allowed.contains(&key)
}

/// Gemini takes Files API URIs and public or signed HTTPS URLs; not YouTube or `gs://`.
fn check_gemini_uri(u: &str) -> Result<(), Error> {
    let un = |m: &str| Err(Error::new(ErrorCode::Unsupported, m));
    if !u.starts_with("https://") {
        return un("Gemini media URIs must be https:// URLs or Files API URIs");
    }
    let host = u[8..].split(['/', '?', '#']).next().unwrap_or("");
    if host.contains("youtube.com") || host.contains("youtu.be") {
        return un("Gemini does not accept YouTube URLs");
    }
    Ok(())
}

/// Gemini's context window, in tokens.
const GEMINI_CONTEXT_TOKENS: usize = 262_144;

/// Lower-bound token estimate (UTF-8 bytes / 4) of text input and questions.
/// Media is not counted, so this only catches requests that are certainly too big.
fn check_gemini_context(req: &DecisionRequest) -> Result<(), Error> {
    let mut bytes = match &req.input {
        Value::String(s) => s.len(),
        other => other.to_string().len(),
    };
    for (id, q) in &req.questions {
        bytes += id.len();
        bytes += match q {
            Question::YesNo {
                instructions,
                yes_description,
                no_description,
            } => {
                instructions.len()
                    + yes_description.as_ref().map_or(0, String::len)
                    + no_description.as_ref().map_or(0, String::len)
            }
            Question::Choice {
                instructions,
                options,
            } => {
                instructions.len()
                    + options
                        .iter()
                        .map(|(k, v)| k.len() + v.len())
                        .sum::<usize>()
            }
            Question::Score {
                instructions,
                levels,
            } => {
                instructions.len()
                    + levels
                        .iter()
                        .map(|l| l.id.len() + l.description.len())
                        .sum::<usize>()
            }
        };
    }
    if bytes / 4 > GEMINI_CONTEXT_TOKENS {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "input and questions exceed Gemini's 262,144-token context",
        ));
    }
    Ok(())
}

/// Decoded size of a base64 string, without decoding it.
fn decoded_len(b64: &str) -> usize {
    let pad = b64.bytes().rev().take_while(|&b| b == b'=').count();
    (b64.len() / 4 * 3).saturating_sub(pad)
}

fn check_gemini_sizes(media: &[Media]) -> Result<(), Error> {
    let too_big = |m: &str| Err(Error::new(ErrorCode::Unsupported, m));
    let total: usize = media.iter().map(|m| decoded_len(&m.data)).sum();
    let pdf: usize = media
        .iter()
        .filter(|m| m.kind() == Some("document"))
        .map(|m| decoded_len(&m.data))
        .sum();
    if total > GEMINI_MAX_INLINE {
        return too_big("Gemini inline data is limited to 100 MB per request");
    }
    if pdf > GEMINI_MAX_PDF {
        return too_big("Gemini inline PDF data is limited to 50 MB per request");
    }
    Ok(())
}

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
    model: Option<String>,
    #[serde(default)]
    answers: BTreeMap<Id, RawAnswer>,
    usage: Option<Usage>,
    prompt_feedback: Option<PromptFeedback>,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(alias = "total_input_tokens")]
    input_tokens: Option<u32>,
    #[serde(alias = "total_cached_tokens")]
    cached_tokens: Option<u32>,
    #[serde(alias = "total_thought_tokens")]
    thought_tokens: Option<u32>,
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
    fn build(
        &self,
        model: &str,
        req: &DecisionRequest,
        cfg: &Config,
    ) -> Result<HttpRequest, Error> {
        if self.preset == Preset::Gemini {
            req.validate_with(IdRule::Gemini)?;
            check_gemini_context(req)?;
        } else {
            req.validate()?;
            req.require_no_gemini_extras()?;
        }
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
        if let (Preset::Gemini, 400, Some(_)) =
            (self.preset, res.status, blocked_by_error(&res.body))
        {
            let reason = blocked_by_error(&res.body);
            return Ok(refused(req, None, Usage::default(), reason));
        }
        if !(200..300).contains(&res.status) {
            return Err(http_error(res));
        }
        let bad = |m: String| Error {
            status: Some(res.status),
            ..Error::new(ErrorCode::BadResponse, m)
        };
        let mut reply: Reply = serde_json::from_slice(&res.body).map_err(|e| bad(e.to_string()))?;
        let usage = reply.usage.take().unwrap_or_default();

        // Input blocked by a safety filter: nothing was evaluated.
        if let Some(f) = reply.prompt_feedback.take() {
            if let Some(reason) = f.block_reason {
                let reason = f.block_reason_message.unwrap_or(reason);
                return Ok(refused(req, reply.model, usage, Some(reason)));
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
            input_tokens: usage.input_tokens,
            cached_tokens: usage.cached_tokens,
            thought_tokens: usage.thought_tokens,
            block_reason: None,
        })
    }
}

/// Every question refused: the input was blocked, nothing was evaluated.
fn refused(
    req: &DecisionRequest,
    model: Option<String>,
    usage: Usage,
    block_reason: Option<String>,
) -> DecisionResponse {
    DecisionResponse {
        model,
        answers: req
            .questions
            .keys()
            .map(|id| (id.clone(), Answer::Refused))
            .collect(),
        input_tokens: usage.input_tokens,
        cached_tokens: usage.cached_tokens,
        thought_tokens: usage.thought_tokens,
        block_reason,
    }
}

/// Gemini's documented HTTP 400 safety shape: `{"error": {"code": "safety", ..}}`.
/// Returns the block reason (the error message, else the code).
fn blocked_by_error(body: &[u8]) -> Option<String> {
    let v = serde_json::from_slice::<Value>(body).ok()?;
    let code = v["error"]["code"].as_str()?;
    GEMINI_BLOCK_CODES
        .contains(&code)
        .then(|| v["error"]["message"].as_str().unwrap_or(code).to_owned())
}
