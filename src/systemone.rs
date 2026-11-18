//! Adapter for the System One wire family: Jev, Clef, Gemini and compatible
//! servers. One shape, four presets.
//!
//! Verified against Gemini's reference (via a third-party mirror). Jev and Clef
//! are assumed to match it; check their docs.

use crate::provider::{http_error, post};
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

    fn max_questions(&self) -> usize {
        match self.preset {
            Preset::Clef => 64,
            Preset::Gemini => 1000,
            _ => usize::MAX,
        }
    }
}

// Request wire types, borrowed so the body is serialised straight to one buffer.
#[derive(Serialize)]
struct Body<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    // Jev and Clef call the input `state`; Gemini calls it `input`.
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<Value>,
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
        if req.questions.len() > self.max_questions() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!("max {} questions per request", self.max_questions()),
            ));
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
        let gemini = self.preset == Preset::Gemini;
        let body = Body {
            // Clef puts the model in the URL.
            model: (self.preset != Preset::Clef).then_some(model),
            state: (!gemini).then_some(&req.input),
            // Gemini takes text; JSON must be sent as a string.
            input: gemini.then(|| match &req.input {
                Value::String(_) => req.input.clone(),
                other => Value::String(other.to_string()),
            }),
            questions,
        };
        let body = serde_json::to_vec(&body)
            .map_err(|e| Error::new(ErrorCode::InvalidRequest, e.to_string()))?;
        let url = self.url(model, cfg)?;
        Ok(if gemini {
            post(url, "x-goog-api-key", false, cfg, body)
        } else {
            post(url, "authorization", true, cfg, body)
        })
    }

    fn parse(&self, req: &DecisionRequest, res: &HttpResponse) -> Result<DecisionResponse, Error> {
        if !(200..300).contains(&res.status) {
            return Err(http_error(res));
        }
        let bad = |m: String| Error {
            status: Some(res.status),
            ..Error::new(ErrorCode::BadResponse, m)
        };
        let mut reply: Reply = serde_json::from_slice(&res.body).map_err(|e| bad(e.to_string()))?;
        let input_tokens = reply.usage.and_then(|u| u.input_tokens);

        // Input blocked by a safety filter: nothing was evaluated.
        if reply.prompt_feedback.and_then(|f| f.block_reason).is_some() {
            let answers = req
                .questions
                .keys()
                .map(|id| (id.clone(), Answer::Refused))
                .collect();
            return Ok(DecisionResponse {
                model: reply.model,
                answers,
                input_tokens,
            });
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
