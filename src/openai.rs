//! OpenAI Decisions adapter (`POST /v1/decisions`, model `gpt-6-luna`).
//!
//! Verified against OpenAI's reference. Images go as base64 data URLs.

use crate::provider::{http_error, post};
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub struct OpenAi;

const MAX_QUESTIONS: usize = 400;
const MAX_LEVELS: usize = 10;

#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    input: Value,
    questions: Vec<Wire<'a>>,
}

// Questions go out as an ordered array; `name` carries our id.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Wire<'a> {
    Predicate {
        name: &'a str,
        instructions: &'a str,
    },
    Choice {
        name: &'a str,
        instructions: &'a str,
        choices: Vec<Value>,
    },
    Score {
        name: &'a str,
        instructions: &'a str,
        levels: Vec<Value>,
    },
}

#[derive(Deserialize)]
struct Reply {
    model: Option<String>,
    answers: Vec<RawAnswer>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u32>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RawAnswer {
    Predicate {
        probability: f32,
    },
    Choice {
        choice: Id,
        confidence: f32,
        probabilities: Vec<ChoiceProb>,
    },
    Score {
        score: f32,
        confidence: f32,
        probabilities: Vec<ScoreProb>,
    },
    Refusal,
}

#[derive(Deserialize)]
struct ChoiceProb {
    value: Id,
    probability: f32,
}

#[derive(Deserialize)]
struct ScoreProb {
    label: Id,
    probability: f32,
}

impl Provider for OpenAi {
    fn build(
        &self,
        model: &str,
        req: &DecisionRequest,
        cfg: &Config,
    ) -> Result<HttpRequest, Error> {
        req.validate()?;
        let unsupported = |m: String| Err(Error::new(ErrorCode::Unsupported, m));
        if req.questions.len() > MAX_QUESTIONS {
            return unsupported(format!("max {MAX_QUESTIONS} questions per request"));
        }
        req.require_images_only()?;
        let input = match (&req.input, req.media.is_empty()) {
            (Value::String(_), true) => req.input.clone(),
            (Value::String(_) | Value::Object(_) | Value::Array(_), _) => {
                let text = match &req.input {
                    Value::String(t) => json!({ "type": "input_text", "text": t }),
                    data => json!({ "type": "input_json", "data": data }),
                };
                let images = req
                    .media
                    .iter()
                    .map(|i| json!({ "type": "input_image", "image_url": i.data_url() }));
                let parts: Vec<Value> = std::iter::once(text).chain(images).collect();
                json!([{ "role": "user", "content": parts }])
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::InvalidRequest,
                    "input must be a string or JSON",
                ))
            }
        };
        let mut questions = Vec::with_capacity(req.questions.len());
        for (id, q) in &req.questions {
            questions.push(match q {
                Question::YesNo { instructions } => Wire::Predicate {
                    name: id,
                    instructions,
                },
                Question::Choice {
                    instructions,
                    options,
                } => Wire::Choice {
                    name: id,
                    instructions,
                    choices: options
                        .iter()
                        .map(|(v, d)| json!({ "value": v, "description": d }))
                        .collect(),
                },
                Question::Score {
                    instructions,
                    levels,
                } => {
                    if levels.len() > MAX_LEVELS {
                        return unsupported(format!("question `{id}`: max {MAX_LEVELS} levels"));
                    }
                    Wire::Score {
                        name: id,
                        instructions,
                        levels: levels
                            .iter()
                            .map(|l| json!({ "label": l.id, "description": l.description }))
                            .collect(),
                    }
                }
            });
        }
        let body = serde_json::to_vec(&Body {
            model,
            input,
            questions,
        })
        .map_err(|e| Error::new(ErrorCode::InvalidRequest, e.to_string()))?;
        Ok(post(
            "https://api.openai.com/v1/decisions".into(),
            "authorization",
            true,
            cfg,
            body,
        ))
    }

    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error> {
        if !(200..300).contains(&res.status) {
            return Err(http_error(res));
        }
        let bad = |m: String| Error {
            status: Some(res.status),
            ..Error::new(ErrorCode::BadResponse, m)
        };
        let reply: Reply = serde_json::from_slice(&res.body).map_err(|e| bad(e.to_string()))?;
        if reply.answers.len() != req.questions.len() {
            return Err(bad(format!(
                "expected {} answers, got {}",
                req.questions.len(),
                reply.answers.len()
            )));
        }

        // Answers come back in question order.
        let mut answers = BTreeMap::new();
        for ((id, q), raw) in req.questions.iter().zip(reply.answers) {
            let answer = match (q, raw) {
                (_, RawAnswer::Refusal) => Answer::Refused,
                (Question::YesNo { .. }, RawAnswer::Predicate { probability }) => {
                    Answer::YesNo { probability }
                }
                (
                    Question::Choice { .. },
                    RawAnswer::Choice {
                        choice,
                        confidence,
                        probabilities,
                    },
                ) => Answer::Choice {
                    choice,
                    confidence: Some(confidence),
                    probabilities: probabilities
                        .into_iter()
                        .map(|p| (p.value, p.probability))
                        .collect(),
                },
                (
                    Question::Score { .. },
                    RawAnswer::Score {
                        score,
                        confidence,
                        probabilities,
                    },
                ) => Answer::Score {
                    value: score,
                    confidence: Some(confidence),
                    probabilities: probabilities
                        .into_iter()
                        .map(|p| (p.label, p.probability))
                        .collect(),
                },
                _ => return Err(bad(format!("answer `{id}` has the wrong type"))),
            };
            answers.insert(id.clone(), answer);
        }
        Ok(DecisionResponse {
            model: reply.model,
            answers,
            input_tokens: reply.usage.and_then(|u| u.input_tokens),
        })
    }
}
