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
    /// Base64 bytes, no `data:` prefix. Empty when `uri` is used.
    #[serde(default)]
    pub data: String,
    /// Public or signed `https://` URL, or a Gemini Files API URI, instead of
    /// `data`. Gemini only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Per-media options, e.g. `resolution`, `fps`, `start_offset`. Gemini
    /// only; other providers return `Unsupported` when this is set.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, Value>,
}

impl Media {
    /// `image`, `audio`, `video` or `document` (PDF, Office, CSV), from the MIME type.
    pub fn kind(&self) -> Option<&'static str> {
        match self.mime_type.split_once('/')?.0 {
            "image" => Some("image"),
            "audio" => Some("audio"),
            "video" => Some("video"),
            _ if DOCUMENT_MIME.contains(&self.mime_type.as_str()) => Some("document"),
            _ => None,
        }
    }

    pub(crate) fn is_image(&self) -> bool {
        self.kind() == Some("image")
    }

    pub(crate) fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime_type, self.data)
    }
}

/// Document MIME types. Only PDF is confirmed by Gemini's guide page; the rest
/// come from its API schema and are untested against the live API.
pub(crate) const DOCUMENT_MIME: [&str; 12] = [
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-powerpoint",
    "application/msword",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.ms-excel",
    "text/csv",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Question {
    YesNo {
        instructions: String,
        /// What "yes" and "no" mean. Gemini only; others return `Unsupported`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        yes_description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        no_description: Option<String>,
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
    /// Tokens served from cache, when the provider reports them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,
    /// Reasoning tokens, when the provider reports them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_tokens: Option<u32>,
    /// Why the input was blocked, when every answer is `Refused` and the
    /// provider says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
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
    /// For providers that take images only.
    pub(crate) fn require_images_only(&self) -> Result<(), Error> {
        match self.media.iter().find(|m| !m.is_image()) {
            Some(m) => Err(Error::new(
                ErrorCode::Unsupported,
                format!("this provider takes images only, not `{}`", m.mime_type),
            )),
            None => Ok(()),
        }
    }

    /// For providers that ignore per-media options and yes/no descriptions.
    pub(crate) fn require_no_gemini_extras(&self) -> Result<(), Error> {
        let un = |m: String| Err(Error::new(ErrorCode::Unsupported, m));
        if self.media.iter().any(|m| m.uri.is_some()) {
            return un("URI media is supported by Gemini only".into());
        }
        if self.media.iter().any(|m| !m.options.is_empty()) {
            return un("per-media options are supported by Gemini only".into());
        }
        let described = self.questions.values().any(|q| {
            matches!(q, Question::YesNo { yes_description, no_description, .. }
                if yes_description.is_some() || no_description.is_some())
        });
        if described {
            return un("yes/no descriptions are supported by Gemini only".into());
        }
        Ok(())
    }

    /// Provider-independent checks. Providers add their own limits.
    pub fn validate(&self) -> Result<(), Error> {
        self.validate_with(IdRule::Strict)
    }

    /// As `validate`, with the id rule the provider allows.
    pub(crate) fn validate_with(&self, rule: IdRule) -> Result<(), Error> {
        let bad = |m: String| Err(Error::new(ErrorCode::InvalidRequest, m));
        if self.questions.is_empty() {
            return bad("no questions".into());
        }
        for m in &self.media {
            match (m.data.is_empty(), &m.uri) {
                (true, None) => return bad("media needs `data` or `uri`".into()),
                (false, Some(_)) => return bad("media takes `data` or `uri`, not both".into()),
                _ => {}
            }
        }
        for (id, q) in &self.questions {
            if !rule.question_id(id) {
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
            if let Some(d) = ids.iter().find(|i| !rule.option_id(i) || !seen.insert(**i)) {
                return bad(format!("question `{id}`: invalid or duplicate id `{d}`"));
            }
        }
        Ok(())
    }
}

/// Which ids a provider accepts.
#[derive(Clone, Copy)]
pub(crate) enum IdRule {
    /// `[a-z0-9_]{1,64}` everywhere.
    Strict,
    /// Gemini: question keys `[A-Za-z0-9_-]{1,128}`; option and level keys any
    /// text up to 128 chars and 512 bytes.
    Gemini,
}

impl IdRule {
    fn question_id(self, s: &str) -> bool {
        match self {
            IdRule::Strict => valid_id(s),
            IdRule::Gemini => {
                (1..=128).contains(&s.len())
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            }
        }
    }

    fn option_id(self, s: &str) -> bool {
        match self {
            IdRule::Strict => valid_id(s),
            IdRule::Gemini => !s.is_empty() && s.chars().count() <= 128 && s.len() <= 512,
        }
    }
}

/// `[a-z0-9_]{1,64}`
fn valid_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
