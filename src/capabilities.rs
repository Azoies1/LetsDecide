//! What a provider can take, as data. Used to validate requests before they
//! are sent, by middleware to route around providers that can't answer, and
//! by custom providers to get the same checks as the built-ins.

use crate::error::{Details, Reason};
use crate::{DecisionRequest, Error, ErrorCode, Question};
use serde::{Deserialize, Serialize};

/// Where the model runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locality {
    /// A vendor's API.
    #[default]
    Cloud,
    /// A server the caller runs (self-hosted, sidecar, LAN).
    Network,
    /// In-process on this device.
    Device,
}

/// Where answer probabilities come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbabilitySource {
    /// The provider's decision API returned them.
    #[default]
    Provider,
    /// Label log-probs from a language model.
    Logprob,
    /// Entailment scores from an NLI classifier.
    Nli,
}

/// Request limits and properties of one provider. `None` means no limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub max_questions: Option<usize>,
    #[serde(default)]
    pub max_options: Option<usize>,
    #[serde(default)]
    pub max_levels: Option<usize>,
    #[serde(default)]
    pub max_media: Option<usize>,
    /// Accepted MIME types; `type/*` matches a whole kind. Empty: no media.
    #[serde(default)]
    pub media: Vec<String>,
    /// Question kinds accepted (`yesno`, `choice`, `score`). Empty: all.
    #[serde(default)]
    pub question_kinds: Vec<String>,
    /// Ids may use `[A-Za-z0-9_-]{1,128}` (Gemini) instead of the strict
    /// `[a-z0-9_]{1,64}`.
    #[serde(default)]
    pub wide_ids: bool,
    #[serde(default)]
    pub probabilities: ProbabilitySource,
    #[serde(default)]
    pub locality: Locality,
}

impl Default for Capabilities {
    /// No limits, no media, all question kinds, cloud.
    fn default() -> Self {
        Self {
            max_questions: None,
            max_options: None,
            max_levels: None,
            max_media: None,
            media: vec![],
            question_kinds: vec![],
            wide_ids: false,
            probabilities: ProbabilitySource::Provider,
            locality: Locality::Cloud,
        }
    }
}

impl Capabilities {
    /// Whether `mime` is in [`media`](Self::media).
    pub fn accepts_media(&self, mime: &str) -> bool {
        self.media.iter().any(|m| match m.strip_suffix("/*") {
            Some(kind) => mime.split_once('/').is_some_and(|(k, _)| k == kind),
            None => m == mime,
        })
    }

    /// Validates the request, then checks it against these capabilities.
    /// Errors are `InvalidRequest` or `Unsupported`, with [`Details`].
    pub fn check(&self, req: &DecisionRequest) -> Result<(), Error> {
        if self.wide_ids {
            req.validate_with(crate::gemini::id_ok)?;
        } else {
            req.validate()?;
        }
        let over = |reason, max: usize, n: usize, what: String| {
            Err(Error::detailed(
                ErrorCode::Unsupported,
                format!("{what}: max {max}, got {n}"),
                Details::new(reason).limit(max, n),
            ))
        };
        let n = req.questions.len();
        if let Some(max) = self.max_questions.filter(|&m| n > m) {
            return over(Reason::TooManyQuestions, max, n, "questions".into());
        }
        if let Some(max) = self.max_media.filter(|&m| req.media.len() > m) {
            return over(Reason::TooManyMedia, max, req.media.len(), "media".into());
        }
        if let Some(m) = req.media.iter().find(|m| !self.accepts_media(&m.mime_type)) {
            return Err(Error::detailed(
                ErrorCode::Unsupported,
                format!("media type `{}` is not accepted", m.mime_type),
                Details::new(Reason::MediaNotAccepted).mime_type(&m.mime_type),
            ));
        }
        for (id, q) in &req.questions {
            let kind = q.kind();
            if !self.question_kinds.is_empty() && !self.question_kinds.iter().any(|k| k == kind) {
                return Err(Error::detailed(
                    ErrorCode::Unsupported,
                    format!("question `{id}`: `{kind}` questions are not supported"),
                    Details::new(Reason::QuestionKind).question(id),
                ));
            }
            let (reason, max, n) = match q {
                Question::Choice { options, .. } => {
                    (Reason::TooManyOptions, self.max_options, options.len())
                }
                Question::Score { levels, .. } => {
                    (Reason::TooManyLevels, self.max_levels, levels.len())
                }
                Question::YesNo { .. } => continue,
            };
            if let Some(max) = max.filter(|&m| n > m) {
                let err = over(reason, max, n, format!("question `{id}`"));
                return err.map_err(|mut e| {
                    if let Some(d) = &mut e.details {
                        d.question = Some(id.clone());
                    }
                    e
                });
            }
        }
        Ok(())
    }
}
