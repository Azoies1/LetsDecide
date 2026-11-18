//! Canonical data model. Vendor terms (`noul`, `predicate`) never appear here.

use crate::{Error, ErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Ids are app-chosen and come back unchanged.
pub type Id = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    /// Text or JSON the questions are asked about.
    pub input: Value,
    pub questions: BTreeMap<Id, Question>,
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
    /// (Jev, Clef, Gemini) report 0-based positions, so it can fall between levels.
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

/// `[a-z0-9_]{1,64}`
fn valid_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
