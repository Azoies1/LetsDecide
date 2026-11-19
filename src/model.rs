//! Canonical data model. Vendor terms (`noul`, `predicate`) never appear here.

use crate::error::{Details, Reason};
use crate::{Error, ErrorCode, ProbabilitySource};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Ids are app-chosen and come back unchanged.
pub type Id = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    /// Text or JSON the questions are asked about.
    pub input: Value,
    /// Shown alongside `input`. Inline only: base64 works on every provider.
    /// Only Gemini takes non-image kinds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<Media>,
    pub questions: BTreeMap<Id, Question>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Media {
    /// e.g. `image/png`, `audio/mp3`, `video/mp4`, `application/pdf`.
    pub mime_type: String,
    /// Base64 bytes, no `data:` prefix.
    pub data: String,
}

impl Media {
    /// `image`, `audio`, `video` or `document` (PDF), from the MIME type.
    pub fn kind(&self) -> Option<&'static str> {
        match self.mime_type.split_once('/')?.0 {
            "image" => Some("image"),
            "audio" => Some("audio"),
            "video" => Some("video"),
            _ if self.mime_type == "application/pdf" => Some("document"),
            _ => None,
        }
    }

    pub(crate) fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime_type, self.data)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Question {
    YesNo {
        instructions: String,
    },
    /// 2..N options, id → description.
    Choice {
        instructions: String,
        options: BTreeMap<Id, String>,
    },
    /// Levels ordered low → high.
    Score {
        instructions: String,
        levels: Vec<Level>,
    },
}

impl Question {
    /// `yesno`, `choice` or `score`: the wire name of the kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::YesNo { .. } => "yesno",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub id: Id,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionResponse {
    /// Model that answered, when the provider reports it.
    pub model: Option<String>,
    pub answers: BTreeMap<Id, Answer>,
    pub input_tokens: Option<u32>,
    /// Where the probabilities come from. Never compare numbers across
    /// sources as if they were the same scale.
    #[serde(default)]
    pub probability_source: ProbabilitySource,
}

/// Probabilities are the provider's numbers, never rescaled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Answer {
    /// P(yes).
    YesNo { probability: f32 },
    /// `confidence`: the provider's own number, if it sends one.
    Choice {
        choice: Id,
        confidence: Option<f32>,
        probabilities: BTreeMap<Id, f32>,
    },
    /// `value`: the provider's score. Systems that average level indices
    /// (Jev, Clef, Gemini; see Google's `create-decision` reference) report 0-based positions, so it can fall between levels.
    Score {
        value: f32,
        confidence: Option<f32>,
        probabilities: BTreeMap<Id, f32>,
    },
    /// A normal answer, not an error: other questions still succeed.
    Refused,
}

impl DecisionRequest {
    /// Provider-independent checks. Providers add their own limits.
    pub fn validate(&self) -> Result<(), Error> {
        let bad = |m: String| Err(Error::new(ErrorCode::InvalidRequest, m));
        if self.questions.is_empty() {
            return bad("no questions".into());
        }
        for (id, q) in &self.questions {
            if !valid_id(id) {
                return bad(format!("invalid question id `{id}`"));
            }
            let ids: Vec<&str> = match q {
                Question::YesNo { .. } => vec![],
                Question::Choice { options, .. } => options.keys().map(String::as_str).collect(),
                Question::Score { levels, .. } => levels.iter().map(|l| l.id.as_str()).collect(),
            };
            if !matches!(q, Question::YesNo { .. }) && ids.len() < 2 {
                return bad(format!(
                    "question `{id}` needs at least 2 options or levels"
                ));
            }
            let mut seen = std::collections::HashSet::new();
            if let Some(d) = ids.iter().find(|i| !valid_id(i) || !seen.insert(**i)) {
                return bad(format!("question `{id}`: invalid or duplicate id `{d}`"));
            }
        }
        Ok(())
    }
}

/// Slack for float rounding in providers' probabilities.
const EPS: f32 = 1e-3;

fn valid_probability(p: f32) -> bool {
    p.is_finite() && (-EPS..=1.0 + EPS).contains(&p)
}

impl DecisionResponse {
    /// Checks the answers fit the request: every question answered once,
    /// answer kinds match, ids are the caller's, probabilities are finite
    /// and in `[0, 1]`. Applied to every provider, built-in or custom, so a
    /// buggy adapter fails with `BadResponse` instead of reaching the app.
    ///
    /// It can't tell whether an answer is *right*, only whether it is
    /// well-formed. Score `value` is only checked to be finite: providers
    /// differ on its range.
    pub fn check(&self, req: &DecisionRequest) -> Result<(), Error> {
        let bad = |m: String, d: Details| Err(Error::detailed(ErrorCode::BadResponse, m, d));
        let number = |q: &str, what: &str, id: Option<&str>| {
            let mut d = Details::new(Reason::InvalidNumber).question(q);
            if let Some(id) = id {
                d = d.id(id);
            }
            bad(format!("answer `{q}`: {what} is not a probability"), d)
        };
        // An unknown id isn't the caller's, so it isn't echoed.
        if self.answers.keys().any(|k| !req.questions.contains_key(k)) {
            return bad(
                "answer for a question that wasn't asked".into(),
                Details::new(Reason::UnexpectedAnswer),
            );
        }
        for (qid, q) in &req.questions {
            let Some(a) = self.answers.get(qid) else {
                return bad(
                    format!("missing answer `{qid}`"),
                    Details::new(Reason::MissingAnswer).question(qid),
                );
            };
            let probs = |probs: &BTreeMap<Id, f32>, ids: &[&str]| -> Result<(), Error> {
                for (k, p) in probs {
                    if !ids.contains(&k.as_str()) {
                        return bad(
                            format!("answer `{qid}`: probability for an unknown id"),
                            Details::new(Reason::UnknownId).question(qid),
                        );
                    }
                    if !valid_probability(*p) {
                        return number(qid, "a probability", Some(k));
                    }
                }
                Ok(())
            };
            let confidence = |c: &Option<f32>| match c {
                Some(c) if !valid_probability(*c) => number(qid, "`confidence`", None),
                _ => Ok(()),
            };
            match (q, a) {
                (_, Answer::Refused) => {}
                (Question::YesNo { .. }, Answer::YesNo { probability }) => {
                    if !valid_probability(*probability) {
                        return number(qid, "`probability`", None);
                    }
                }
                (
                    Question::Choice { options, .. },
                    Answer::Choice {
                        choice,
                        confidence: c,
                        probabilities,
                    },
                ) => {
                    let ids: Vec<&str> = options.keys().map(String::as_str).collect();
                    if !ids.contains(&choice.as_str()) {
                        return bad(
                            format!("answer `{qid}`: `choice` is not one of the options"),
                            Details::new(Reason::UnknownId).question(qid),
                        );
                    }
                    confidence(c)?;
                    probs(probabilities, &ids)?;
                }
                (
                    Question::Score { levels, .. },
                    Answer::Score {
                        value,
                        confidence: c,
                        probabilities,
                    },
                ) => {
                    if !value.is_finite() {
                        return number(qid, "`value`", None);
                    }
                    let ids: Vec<&str> = levels.iter().map(|l| l.id.as_str()).collect();
                    confidence(c)?;
                    probs(probabilities, &ids)?;
                }
                _ => {
                    return bad(
                        format!("answer `{qid}` is not a `{}` answer", q.kind()),
                        Details::new(Reason::WrongAnswerKind).question(qid),
                    )
                }
            }
        }
        Ok(())
    }
}

/// `[a-z0-9_]{1,64}`
fn valid_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
