//! Local validation, so a malformed request fails here rather than as a 422.

use crate::question::Question;
use crate::request::Request;

/// One thing wrong with a request, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// A JSON path into the request body, such as `questions.tone.criteria`.
    pub path: String,
    /// What is wrong at that path.
    pub problem: String,
}

/// Everything wrong with a request, reported together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    /// The problems found, in the order they appear in the body.
    pub issues: Vec<Issue>,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "request rejected before sending ({} problem(s))",
            self.issues.len()
        )?;
        for issue in &self.issues {
            write!(f, "\n  - {}: {}", issue.path, issue.problem)?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationError {}

/// The documented shape limits, checked before a request is sent.
///
/// The defaults match what the API documents today. Server limits can move, so
/// every bound can be lifted with [`Limits::unbounded`] if the docs change
/// before this crate does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Most options one Choice may offer. Documented as 255.
    pub max_choice_options: Option<usize>,
    /// Fewest levels a Score should define. Documented as 2.
    pub min_score_levels: Option<usize>,
    /// Most levels a Score may define. Documented as 10.
    pub max_score_levels: Option<usize>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_choice_options: Some(255),
            min_score_levels: Some(2),
            max_score_levels: Some(10),
        }
    }
}

impl Limits {
    /// Drops every documented bound.
    ///
    /// Structural checks still run: a request with no questions, or a Score
    /// with no levels, cannot produce a meaningful answer either way.
    pub fn unbounded() -> Self {
        Self {
            max_choice_options: None,
            min_score_levels: None,
            max_score_levels: None,
        }
    }
}

/// Checks a request against `limits`, collecting every problem found.
pub fn check(request: &Request, limits: Limits) -> Result<(), ValidationError> {
    let mut issues = Vec::new();

    if request.model.trim().is_empty() {
        issues.push(Issue {
            path: "model".to_owned(),
            problem: "must name a model or alias, such as jev-latest".to_owned(),
        });
    }

    if request.questions.is_empty() {
        issues.push(Issue {
            path: "questions".to_owned(),
            problem: "must contain at least one question".to_owned(),
        });
    }

    for (id, question) in &request.questions {
        let path = format!("questions.{id}");
        if id.trim().is_empty() {
            issues.push(Issue {
                path: path.clone(),
                problem: "question id must not be blank".to_owned(),
            });
        }
        check_question(&path, question, limits, &mut issues);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(ValidationError { issues })
    }
}

fn check_question(path: &str, question: &Question, limits: Limits, issues: &mut Vec<Issue>) {
    match question {
        Question::Noul(_) => {}
        Question::Choice(choice) => {
            if choice.criteria.is_empty() {
                issues.push(Issue {
                    path: format!("{path}.criteria"),
                    problem: "a choice must offer at least one option".to_owned(),
                });
            }
            if let Some(max) = limits.max_choice_options
                && choice.criteria.len() > max
            {
                issues.push(Issue {
                    path: format!("{path}.criteria"),
                    problem: format!(
                        "{} options exceeds the limit of {max}",
                        choice.criteria.len()
                    ),
                });
            }
            for (index, name) in choice.criteria.keys().enumerate() {
                if name.trim().is_empty() {
                    issues.push(Issue {
                        path: format!("{path}.criteria[{index}]"),
                        problem: "option name must not be blank".to_owned(),
                    });
                }
            }
        }
        Question::Score(score) => {
            let levels = score.criteria.len();
            if levels == 0 {
                issues.push(Issue {
                    path: format!("{path}.criteria"),
                    problem: "a score must define at least one level".to_owned(),
                });
                return;
            }
            if let Some(min) = limits.min_score_levels
                && levels < min
            {
                issues.push(Issue {
                    path: format!("{path}.criteria"),
                    problem: format!(
                        "{levels} level(s) is below the recommended minimum of {min}; \
                         a one-level scale has nothing to weigh"
                    ),
                });
            }
            if let Some(max) = limits.max_score_levels
                && levels > max
            {
                issues.push(Issue {
                    path: format!("{path}.criteria"),
                    problem: format!("{levels} levels exceeds the limit of {max}"),
                });
            }
        }
    }
}
