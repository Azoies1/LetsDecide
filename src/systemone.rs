//! Adapter for the System One wire shape (Jev, Clef, compatible servers).
//!
//! Wire format is assumed from the spec; verify against provider docs.

use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// One adapter, three presets: `jev`, `clef`, `systemone` (custom base URL).
pub struct SystemOne {
    preset: Preset,
}

#[derive(Clone, Copy)]
enum Preset {
    Jev,
    Clef,
    Custom,
}

impl SystemOne {
    pub fn preset(name: &str) -> Option<Self> {
        let preset = match name {
            "jev" => Preset::Jev,
            "clef" => Preset::Clef,
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

    /// Clef: 64 questions per request.
    fn max_questions(&self) -> usize {
        match self.preset {
            Preset::Clef => 64,
            _ => usize::MAX,
        }
    }
}

// Request wire types, borrowed so the body is serialised straight to one buffer.
#[derive(Serialize)]
struct Body<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    state: &'a Value,
    questions: BTreeMap<&'a str, Wire<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Wire<'a> {
    Noul {
        prompt: &'a str,
    },
    Choice {
        prompt: &'a str,
        criteria: &'a BTreeMap<Id, String>,
    },
    Score {
        prompt: &'a str,
        levels: &'a [Level],
    },
}

// Response wire types.
#[derive(Deserialize)]
struct Reply {
    model: String,
    answers: BTreeMap<Id, RawAnswer>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct RawAnswer {
    probability: Option<f32>,
    choice: Option<Id>,
    value: Option<f32>,
    #[serde(default)]
    probabilities: BTreeMap<Id, f32>,
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
                    Question::YesNo { instructions } => Wire::Noul {
                        prompt: instructions,
                    },
                    Question::Choice {
                        instructions,
                        options,
                    } => Wire::Choice {
                        prompt: instructions,
                        criteria: options,
                    },
                    Question::Score {
                        instructions,
                        levels,
                    } => Wire::Score {
                        prompt: instructions,
                        levels,
                    },
                };
                (id.as_str(), wire)
            })
            .collect();
        let body = Body {
            // Clef puts the model in the URL.
            model: (!matches!(self.preset, Preset::Clef)).then_some(model),
            state: &req.input,
            questions,
        };
        let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
        if let Some(key) = &cfg.api_key {
            headers.push(("authorization".to_owned(), format!("Bearer {key}")));
        }
        Ok(HttpRequest {
            method: "POST",
            url: self.url(model, cfg)?,
            headers,
            body: serde_json::to_vec(&body)
                .map_err(|e| Error::new(ErrorCode::InvalidRequest, e.to_string()))?,
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

        let mut answers = BTreeMap::new();
        for (id, q) in &req.questions {
            let raw = reply
                .answers
                .remove(id)
                .ok_or_else(|| bad(format!("missing answer `{id}`")))?;
            let missing = |f: &str| bad(format!("answer `{id}` missing `{f}`"));
            let answer = match q {
                Question::YesNo { .. } => Answer::YesNo {
                    probability: raw.probability.ok_or_else(|| missing("probability"))?,
                },
                Question::Choice { .. } => Answer::Choice {
                    choice: raw.choice.ok_or_else(|| missing("choice"))?,
                    probabilities: raw.probabilities,
                },
                Question::Score { .. } => Answer::Score {
                    value: raw.value.ok_or_else(|| missing("value"))?,
                    probabilities: raw.probabilities,
                },
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

fn http_error(res: &HttpResponse) -> Error {
    let code = match res.status {
        401 | 403 => ErrorCode::Auth,
        429 => ErrorCode::RateLimited,
        500..=599 => ErrorCode::ProviderUnavailable,
        _ => ErrorCode::InvalidRequest,
    };
    let retry_after_ms = res
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, v)| v.trim().parse::<u64>().ok())
        .map(|s| s * 1000);
    Error {
        status: Some(res.status),
        retry_after_ms,
        ..Error::new(code, format!("HTTP {}", res.status))
    }
}
