//! The System One wire family: Jev, Clef, Gemini and compatible servers.
//! What differs between them is data in their [`Dialect`]
//! (`spec/dialects/*.json`): endpoint, auth, input field, media encoding,
//! envelope, block codes and limits.
//!
//! Verified against: Jev's OpenAPI spec (api.typesafe.ai/openapi.json), Clef's
//! Cloudflare docs, and Google's official Gemini Decisions pages (read
//! 2026-11-19):
//! - <https://aistudio.google.com/api-reference/decisions/create-decision>
//! - <https://aistudio.google.com/docs/decisions-api>
//!
//! Checked live (2026-11-19): Gemini replies carry no top-level `model` (so
//! `DecisionResponse.model` is `None`; it is still parsed if present). Still
//! unverified: which of the two documented safety-block shapes (200 +
//! `prompt_feedback.block_reason`, or HTTP 400 + `error.code`) applies when.
//! Both are handled.

use crate::dialect::{Dialect, InputField, MediaEncoding, JEV_IMAGE_HEADER};
use crate::error::safe_serde_message;
use crate::gemini;
use crate::model::confidence;
use crate::provider::encode;
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// Puts input and media where the dialect expects them.
fn attach_input<'a>(
    d: &Dialect,
    req: &'a DecisionRequest,
    body: &mut Body<'a>,
) -> Result<(), Error> {
    // Media types and counts were checked against the capabilities already.
    let media = &req.media;
    let text = || match &req.input {
        Value::String(_) => req.input.clone(),
        other => Value::String(other.to_string()),
    };
    let mut value: Cow<'a, Value> = match d.input_field {
        InputField::State => Cow::Borrowed(&req.input),
        InputField::Input => Cow::Owned(text()),
    };
    match d.media_encoding {
        MediaEncoding::None | MediaEncoding::ContentParts => {}
        MediaEncoding::TypedBlocks => {
            if !media.is_empty() {
                value = Cow::Owned(gemini::blocks(value.into_owned(), media)?);
            }
        }
        MediaEncoding::DataUrlList => body.images = media.iter().map(Media::data_url).collect(),
        MediaEncoding::JevAssets if !media.is_empty() => {
            // Placeholders in the input are swapped for the assets server-side.
            let mut refs = vec![];
            for (n, img) in media.iter().enumerate() {
                let format = img
                    .mime_type
                    .strip_prefix("image/")
                    .unwrap_or(&img.mime_type);
                let key = format!("letsdecide-image-{n}");
                let asset = json!({ "type": "image", "format": format, "data_base64": img.data });
                body.assets.insert(key.clone(), asset);
                refs.push(key);
            }
            value = Cow::Owned(match value.into_owned() {
                Value::String(t) => Value::String(format!("{t}\n{}", refs.join("\n"))),
                other => Value::Array(
                    std::iter::once(other)
                        .chain(refs.into_iter().map(Value::String))
                        .collect(),
                ),
            });
        }
        MediaEncoding::JevAssets => {}
    }
    match d.input_field {
        InputField::State => body.state = Some(value),
        InputField::Input => body.input = Some(value),
    }
    Ok(())
}

// Request wire types, borrowed so the body is serialised straight to one buffer.
#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    // Jev and Clef call the input `state`; Gemini calls it `input`.
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<Cow<'a, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<Cow<'a, Value>>,
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

pub(crate) fn build(
    d: &Dialect,
    model: &str,
    req: &DecisionRequest,
    cfg: &Config,
) -> Result<HttpRequest, Error> {
    d.capabilities.check(req)?;
    req.require_portable(
        d.media_encoding == MediaEncoding::TypedBlocks,
        false,
        d.capabilities.yesno_descriptions,
    )?;
    let questions =
        req.questions
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
    attach_input(d, req, &mut body)?;
    let body = encode(&body, req)?;
    let url = d.url(model, cfg)?;
    let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
    headers.extend(d.credential(cfg)?);
    if !req.media.is_empty() && d.media_encoding == MediaEncoding::JevAssets {
        headers.push((JEV_IMAGE_HEADER.0.into(), JEV_IMAGE_HEADER.1.into()));
    }
    let mut http = HttpRequest {
        method: "POST",
        url,
        headers,
        body,
    };
    apply_headers(&mut http, cfg)?;
    Ok(http)
}

pub(crate) fn parse(
    d: &Dialect,
    req: &DecisionRequest,
    res: &HttpResponse,
) -> Result<DecisionResponse, Error> {
    if let Some(reason) = d.blocked(res) {
        return Ok(refused(req, None, None, Some(reason)));
    }
    if !(200..300).contains(&res.status) {
        return Err(http_error(res));
    }
    let bad = |m: String| Error {
        status: Some(res.status),
        ..Error::new(ErrorCode::BadResponse, m)
    };
    let v: Value = serde_json::from_slice(&res.body)
        .map_err(|e| bad(safe_serde_message("response body", &e)))?;
    let mut reply: Reply = serde_json::from_value(d.unwrap_envelope(v))
        .map_err(|e| bad(safe_serde_message("response body", &e)))?;
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
