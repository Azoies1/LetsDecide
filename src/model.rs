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
    /// Escape hatch: extra top-level body fields, sent as-is (e.g. a new
    /// provider option not modelled yet). A key the adapter already sets is
    /// rejected rather than overwritten.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub provider_extras: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Media {
    /// e.g. `image/png`, `audio/mp3`, `video/mp4`, `application/pdf`.
    pub mime_type: String,
    /// Base64 bytes, no `data:` prefix. Empty when `uri` is used.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub data: String,
    /// URL instead of `data`: `https://` or a Files API URI on Gemini; a
    /// public `http(s)://` image URL on OpenAI. Others return `Unsupported`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Per-media options (`resolution`, `fps`, ...), passed through.
    /// Gemini only; others return `Unsupported`.
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

    pub(crate) fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime_type, self.data)
    }

    /// The `uri` if set, else a base64 data URL.
    pub(crate) fn url(&self) -> String {
        self.uri.clone().unwrap_or_else(|| self.data_url())
    }
}

/// Documents. Only PDF is confirmed live; the rest are from Gemini's API schema.
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
        /// What yes and no mean. Gemini only.
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
    /// `confidence`: see `confidence_source`.
    Choice {
        choice: Id,
        confidence: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence_source: Option<ConfidenceSource>,
        probabilities: BTreeMap<Id, f32>,
    },
    /// `value`: the provider's score. Systems that average level indices
    /// (Jev, Clef, Gemini; see Google's `create-decision` reference) report 0-based positions, so it can fall between levels.
    Score {
        value: f32,
        confidence: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence_source: Option<ConfidenceSource>,
        probabilities: BTreeMap<Id, f32>,
    },
    /// A normal answer, not an error: other questions still succeed.
    Refused,
}

/// Where `confidence` came from. Each provider computes its own number, so
/// confidences are not comparable across providers either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfidenceSource {
    /// Sent by the provider.
    Provider,
    /// Not sent; taken from `probabilities`: the chosen option's (choice) or
    /// the most likely level's (score). Nothing is rescaled.
    Probabilities,
}

/// The provider's confidence, or one read off the probabilities.
pub(crate) fn confidence(
    provider: Option<f32>,
    chosen: Option<f32>,
) -> (Option<f32>, Option<ConfidenceSource>) {
    match (provider, chosen) {
        (Some(c), _) => (Some(c), Some(ConfidenceSource::Provider)),
        (None, Some(p)) => (Some(p), Some(ConfidenceSource::Probabilities)),
        (None, None) => (None, None),
    }
}

impl DecisionRequest {
    /// For providers without Gemini-only features: URIs, media options, yes/no
    /// descriptions. `image_urls`: the provider takes public image URLs.
    pub(crate) fn require_portable(&self, image_urls: bool) -> Result<(), Error> {
        let un = |m: String| Err(Error::new(ErrorCode::Unsupported, m));
        for u in self.media.iter().filter_map(|m| m.uri.as_deref()) {
            if !image_urls {
                return un("URI media is supported by Gemini and OpenAI (images) only".into());
            }
            if !(u.starts_with("https://") || u.starts_with("http://")) {
                return un("media `uri` must be a public http(s) URL here".into());
            }
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
        self.validate_with(valid_id)
    }

    /// As `validate`, with the provider's id rule (applies to every id).
    pub(crate) fn validate_with(&self, valid_id: fn(&str) -> bool) -> Result<(), Error> {
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
                        ..
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
                        ..
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
pub(crate) fn valid_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
