//! Combines several models' answers to one request. Pure: the host calls the
//! members (concurrently or not) and passes their outcomes here.
//! See `docs/concurrent-models.md`.

use crate::error::{Details, Reason};
use crate::{
    Answer, ConfidenceSource, DecisionRequest, DecisionResponse, Error, ErrorCode, Id,
    ProbabilitySource, Question,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What happened when the host called one member.
#[derive(Debug, Clone)]
pub enum Outcome {
    Answered(DecisionResponse),
    Failed(Error),
    /// Not called, e.g. its capabilities can't take the request. Doesn't
    /// count toward quorum.
    Skipped,
}

#[derive(Debug, Clone)]
pub struct Member {
    /// `"provider/model"`; the prefix goes into error details.
    pub model: String,
    pub weight: f32,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// Weighted majority of each member's top answer. Valid across any
    /// `probability_source`.
    #[default]
    Vote,
    /// Weighted mean of probabilities. Only for members with the same
    /// `probability_source`.
    Mean,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    /// Minimum members that must answer.
    pub quorum: usize,
    pub merge: Method,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            quorum: 1,
            merge: Method::Vote,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Merged {
    #[serde(flatten)]
    pub response: DecisionResponse,
    /// Per question: weighted share of voting members whose top answer is
    /// the merged one. Members that refused don't vote.
    pub agreement: BTreeMap<Id, f32>,
    /// Questions where the voting members were not unanimous.
    pub disagreed: Vec<Id>,
}

fn prefix(model: &str) -> &str {
    model.split_once('/').map_or(model, |(p, _)| p)
}

/// A member's top answer as an id: `yes`/`no`, the choice, or the most
/// likely level. `None` when it refused or a score has no probabilities.
fn top(q: &Question, a: &Answer) -> Option<Id> {
    match (q, a) {
        (_, Answer::YesNo { probability }) => {
            Some(if *probability >= 0.5 { "yes" } else { "no" }.into())
        }
        (_, Answer::Choice { choice, .. }) => Some(choice.clone()),
        (Question::Score { levels, .. }, Answer::Score { probabilities, .. }) => levels
            .iter()
            .filter_map(|l| probabilities.get(&l.id).map(|p| (l, *p)))
            .fold(None::<(&crate::Level, f32)>, |best, (l, p)| match best {
                Some((_, bp)) if bp >= p => best,
                _ => Some((l, p)),
            })
            .map(|(l, _)| l.id.clone()),
        _ => None,
    }
}

/// Highest weight wins; ties go to the label voted for first, so member
/// order (the config's order) breaks ties.
fn winner(votes: &[(Id, f32)]) -> Option<Id> {
    let mut tally: Vec<(Id, f32)> = vec![];
    for (id, w) in votes {
        match tally.iter_mut().find(|(t, _)| t == id) {
            Some((_, t)) => *t += w,
            None => tally.push((id.clone(), *w)),
        }
    }
    tally
        .into_iter()
        .fold(None::<(Id, f32)>, |best, (id, w)| match best {
            Some((_, bw)) if bw >= w => best,
            _ => Some((id, w)),
        })
        .map(|(id, _)| id)
}

fn ids(q: &Question) -> Vec<Id> {
    match q {
        Question::YesNo { .. } => vec!["yes".into(), "no".into()],
        Question::Choice { options, .. } => options.keys().cloned().collect(),
        Question::Score { levels, .. } => levels.iter().map(|l| l.id.clone()).collect(),
    }
}

/// Weighted mean of each member's distribution over `ids`.
fn mean_dist(voters: &[(&Answer, f32)], ids: &[Id]) -> BTreeMap<Id, f32> {
    let total: f32 = voters.iter().map(|(_, w)| w).sum();
    let mut out: BTreeMap<Id, f32> = ids.iter().map(|i| (i.clone(), 0.0)).collect();
    for (a, w) in voters {
        let dist: BTreeMap<Id, f32> = match a {
            Answer::YesNo { probability } => [
                ("yes".into(), *probability),
                ("no".into(), 1.0 - probability),
            ]
            .into(),
            Answer::Choice { probabilities, .. } | Answer::Score { probabilities, .. } => {
                probabilities.clone()
            }
            Answer::Refused => continue,
        };
        for (id, p) in dist {
            *out.entry(id).or_default() += p * w / total;
        }
    }
    out
}

/// Builds the merged answer from a distribution over the question's ids.
fn answer(q: &Question, dist: BTreeMap<Id, f32>, choice: Id, method: Method) -> Answer {
    let conf = dist.get(&choice).copied();
    match q {
        Question::YesNo { .. } => Answer::YesNo {
            probability: dist["yes"],
        },
        Question::Choice { .. } => Answer::Choice {
            choice,
            confidence: conf,
            confidence_source: conf.map(|_| ConfidenceSource::Probabilities),
            probabilities: dist,
        },
        Question::Score { levels, .. } => {
            // 0-based level index (ADR 0004): the winner's index for a vote,
            // the expected index of the mean distribution for `mean`.
            let value = match method {
                Method::Vote => levels.iter().position(|l| l.id == choice).unwrap_or(0) as f32,
                Method::Mean => levels
                    .iter()
                    .enumerate()
                    .map(|(i, l)| i as f32 * dist[&l.id])
                    .sum(),
            };
            Answer::Score {
                value,
                confidence: conf,
                confidence_source: conf.map(|_| ConfidenceSource::Probabilities),
                probabilities: dist,
            }
        }
    }
}

/// Merges member outcomes. Every answered member is
/// [checked](DecisionResponse::check) first; one that fails counts as failed.
pub fn merge(req: &DecisionRequest, members: &[Member], rules: &Rules) -> Result<Merged, Error> {
    let invalid = |m: &str, f: &str| {
        Err(Error::detailed(
            ErrorCode::InvalidRequest,
            m,
            Details::new(Reason::InvalidConfig).field(f),
        ))
    };
    if rules.quorum == 0 {
        return invalid("`quorum` must be at least 1", "quorum");
    }
    if members
        .iter()
        .any(|m| !(m.weight.is_finite() && m.weight > 0.0))
    {
        return invalid("member `weight` must be a positive number", "weight");
    }

    let mut answered: Vec<(&Member, &DecisionResponse)> = vec![];
    let mut last_err: Option<(Error, &str)> = None;
    for m in members {
        match &m.outcome {
            Outcome::Answered(r) => match r.check(req) {
                Ok(()) => answered.push((m, r)),
                Err(e) => last_err = Some((e, prefix(&m.model))),
            },
            Outcome::Failed(e) => last_err = Some((e.clone(), prefix(&m.model))),
            Outcome::Skipped => {}
        }
    }

    if answered.len() < rules.quorum {
        // Keep the last failure's code, status and retry hint, so `retry`
        // and `fallback` treat the panel like the member that failed.
        let (base, failed) =
            last_err.unwrap_or_else(|| (Error::new(ErrorCode::Unsupported, ""), ""));
        let mut d = Details::new(Reason::BelowQuorum).limit(rules.quorum, answered.len());
        d.provider = (!failed.is_empty()).then(|| failed.to_owned());
        return Err(Error {
            message: format!(
                "{} of {} required members answered",
                answered.len(),
                rules.quorum
            ),
            details: Some(Box::new(d)),
            ..base
        });
    }

    let sources: Vec<ProbabilitySource> =
        answered.iter().map(|(_, r)| r.probability_source).collect();
    let source = match rules.merge {
        Method::Vote => ProbabilitySource::Vote,
        Method::Mean => {
            if let Some((m, _)) = answered
                .iter()
                .find(|(_, r)| r.probability_source != sources[0])
            {
                let mut d = Details::new(Reason::MixedProbabilitySource);
                d.provider = Some(prefix(&m.model).to_owned());
                return Err(Error::detailed(
                    ErrorCode::Unsupported,
                    "`mean` needs members with the same `probability_source`; use `vote`",
                    d,
                ));
            }
            sources[0]
        }
    };

    let mut answers = BTreeMap::new();
    let mut agreement = BTreeMap::new();
    let mut disagreed = vec![];
    for (qid, q) in &req.questions {
        let voters: Vec<(&Answer, f32, Id)> = answered
            .iter()
            .filter_map(|(m, r)| {
                let a = &r.answers[qid];
                top(q, a).map(|t| (a, m.weight, t))
            })
            .collect();
        let votes: Vec<(Id, f32)> = voters.iter().map(|(_, w, t)| (t.clone(), *w)).collect();
        let Some(vote_winner) = winner(&votes) else {
            answers.insert(qid.clone(), Answer::Refused);
            continue;
        };
        let total: f32 = votes.iter().map(|(_, w)| w).sum();
        let ids = ids(q);
        let (dist, choice) = match rules.merge {
            Method::Vote => {
                let mut dist: BTreeMap<Id, f32> = ids.iter().map(|i| (i.clone(), 0.0)).collect();
                for (id, w) in &votes {
                    *dist.get_mut(id).expect("checked id") += w / total;
                }
                (dist, vote_winner)
            }
            Method::Mean => {
                let pairs: Vec<(&Answer, f32)> = voters.iter().map(|(a, w, _)| (*a, *w)).collect();
                let dist = mean_dist(&pairs, &ids);
                let choice = ids
                    .iter()
                    .fold(None::<&Id>, |b, i| match b {
                        Some(b) if dist[b] >= dist[i] => Some(b),
                        _ => Some(i),
                    })
                    .expect("at least 2 ids")
                    .clone();
                (dist, choice)
            }
        };
        let agree: f32 = votes
            .iter()
            .filter(|(t, _)| *t == choice)
            .map(|(_, w)| w)
            .sum::<f32>()
            / total;
        agreement.insert(qid.clone(), agree);
        if votes.iter().any(|(t, _)| *t != votes[0].0) {
            disagreed.push(qid.clone());
        }
        answers.insert(qid.clone(), answer(q, dist, choice, rules.merge));
    }

    let all_refused = answers.values().all(|a| matches!(a, Answer::Refused));
    let tokens: Vec<u32> = answered
        .iter()
        .filter_map(|(_, r)| r.input_tokens)
        .collect();
    Ok(Merged {
        response: DecisionResponse {
            model: None,
            answers,
            input_tokens: (!tokens.is_empty()).then(|| tokens.iter().sum()),
            probability_source: source,
            block_reason: if all_refused {
                answered.iter().find_map(|(_, r)| r.block_reason.clone())
            } else {
                None
            },
        },
        agreement,
        disagreed,
    })
}
