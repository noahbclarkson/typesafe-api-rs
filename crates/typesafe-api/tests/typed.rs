//! The typed layer: what the derives generate, and how it reads answers back.

#![cfg(feature = "derive")]

use serde_json::json;
use typesafe_api::{
    ChoiceOf, Entry, Evaluation, Gate, Levels, NoulAnswer, Options, Question, Response, ScoreOf,
};

#[derive(Options, Debug, PartialEq)]
enum Department {
    /// Payments, invoicing, refunds
    Billing,
    /// Bugs, outages, integrations
    Technical,
    #[options(name = "none_of_the_above", describe = "Fits no other option")]
    Other,
}

#[derive(Options, Debug, PartialEq)]
#[options(rename_all = "kebab-case")]
enum ReturnReason {
    /// The item does not fit
    WrongSize,
    /// A different product was delivered
    WrongItem,
    /// The name speaks for itself
    Damaged,
}

#[derive(Options, Debug, PartialEq)]
enum Tolerant {
    /// A known outcome
    Known,
    #[options(unknown, describe = "Anything else")]
    Fallback,
}

#[derive(Levels, Debug, PartialEq)]
enum Severity {
    /// Cosmetic; no impact to functionality
    Cosmetic,
    /// Broken or degraded feature, but workaround exists
    Degraded,
    /// Blocking issue; no workaround exists
    Blocking,
}

#[derive(Evaluation, Debug)]
struct Triage {
    /// The message conveys urgency
    is_urgent: NoulAnswer,

    #[question(
        id = "refund",
        instructions = "Does the customer ask for money back?",
        yes = "Directly asks for a refund or credit",
        no = "A billing question with no requested remedy"
    )]
    refund_requested: NoulAnswer,

    /// Which team should handle this?
    department: ChoiceOf<Department>,

    /// How severe is the reported issue?
    severity: ScoreOf<Severity>,

    /// An answer that may not come back
    experimental: Option<NoulAnswer>,
}

fn response() -> Response {
    serde_json::from_value(json!({
        "model": "jev-1.13.0",
        "answers": {
            "is_urgent": { "type": "noul", "noul": 0.91 },
            "refund": { "type": "noul", "noul": 0.12 },
            "department": {
                "type": "choice",
                "choice": "technical",
                "confidence": 0.61,
                "probabilities": {
                    "billing": 0.3, "technical": 0.65, "none_of_the_above": 0.05
                }
            },
            "severity": {
                "type": "score",
                "score": 1.43,
                "confidence": 0.35,
                "legend": {
                    "0": "Cosmetic; no impact to functionality",
                    "1": "Broken or degraded feature, but workaround exists",
                    "2": "Blocking issue; no workaround exists"
                },
                "probabilities": { "0": 0.0, "1": 0.57, "2": 0.43 }
            }
        },
        "usage": { "input_tokens": 1, "output_tokens": 1 }
    }))
    .unwrap()
}

#[test]
fn option_names_follow_the_rename_rule_and_explicit_overrides() {
    assert_eq!(
        Department::variants(),
        ["billing", "technical", "none_of_the_above"]
    );
    assert_eq!(
        ReturnReason::variants(),
        ["wrong-size", "wrong-item", "damaged"]
    );
    assert_eq!(Department::Other.option_name(), "none_of_the_above");
}

#[test]
fn doc_comments_become_the_criteria_the_model_sees() {
    let criteria = Department::criteria();
    assert_eq!(criteria.len(), 3);
    assert_eq!(
        criteria["billing"],
        Some(Entry::text("Payments, invoicing, refunds"))
    );
    assert_eq!(
        criteria["none_of_the_above"],
        Some(Entry::text("Fits no other option"))
    );
    assert_eq!(
        Severity::criteria(),
        vec![
            Entry::text("Cosmetic; no impact to functionality"),
            Entry::text("Broken or degraded feature, but workaround exists"),
            Entry::text("Blocking issue; no workaround exists"),
        ]
    );
}

#[test]
fn an_unknown_option_is_an_error_naming_the_variants() {
    let error = Department::from_option("marketing").unwrap_err();
    assert_eq!(error.answer, "marketing");
    assert!(error.to_string().contains("billing"), "{error}");
    assert!(error.to_string().contains("none_of_the_above"), "{error}");
}

#[test]
fn a_catch_all_variant_absorbs_an_unknown_option() {
    assert_eq!(Tolerant::from_option("known").unwrap(), Tolerant::Known);
    assert_eq!(
        Tolerant::from_option("something new").unwrap(),
        Tolerant::Fallback
    );
}

#[test]
fn levels_are_numbered_by_declaration_order() {
    assert_eq!(Severity::count(), 3);
    assert_eq!(Severity::Cosmetic.level(), 0);
    assert_eq!(Severity::Blocking.level(), 2);
    assert_eq!(Severity::from_level(1), Some(Severity::Degraded));
    assert_eq!(Severity::from_level(9), None);
}

#[test]
fn the_evaluation_struct_declares_the_questions() {
    let questions = Triage::questions();
    assert_eq!(
        questions.keys().collect::<Vec<_>>(),
        [
            "is_urgent",
            "refund",
            "department",
            "severity",
            "experimental"
        ]
    );

    let Question::Noul(refund) = &questions["refund"] else {
        panic!("expected a noul")
    };
    let criteria = refund.criteria.as_ref().unwrap();
    assert_eq!(
        criteria.yes,
        Some(Entry::text("Directly asks for a refund or credit"))
    );
    assert_eq!(
        criteria.no,
        Some(Entry::text("A billing question with no requested remedy"))
    );

    let Question::Choice(department) = &questions["department"] else {
        panic!("expected a choice")
    };
    assert_eq!(department.criteria.len(), 3);
    assert_eq!(
        department.instructions,
        Some(Entry::text("Which team should handle this?"))
    );

    let Question::Score(severity) = &questions["severity"] else {
        panic!("expected a score")
    };
    assert_eq!(severity.criteria.len(), 3);
}

#[test]
fn the_evaluation_struct_receives_the_answers() {
    let triage: Triage = response().extract().unwrap();

    assert!(triage.is_urgent.is_yes(0.9));
    assert!(!triage.refund_requested.is_yes(0.5));

    assert_eq!(triage.department.value, Department::Technical);
    assert!((triage.department.probability(&Department::Billing) - 0.3).abs() < 1e-9);
    assert_eq!(
        triage.department.runners_up(0.25),
        vec![(Department::Billing, 0.3)]
    );
    assert_eq!(triage.department.gate(0.4, 0.8), Gate::Medium);

    assert_eq!(triage.severity.nearest(), Some(Severity::Degraded));
    assert_eq!(triage.severity.top_level(), 2);
    assert!((triage.severity.normalized() - 0.715).abs() < 1e-9);
    assert!((triage.severity.probability(&Severity::Blocking) - 0.43).abs() < 1e-9);
    assert_eq!(
        triage.severity.describe(&Severity::Cosmetic),
        Some(&Entry::text("Cosmetic; no impact to functionality"))
    );

    // The field is Option, and the response omitted it.
    assert!(triage.experimental.is_none());
}

#[test]
fn an_option_outside_the_enum_is_reported_against_the_question_id() {
    let response: Response = serde_json::from_value(json!({
        "model": "jev-1.13.0",
        "answers": {
            "department": {
                "type": "choice",
                "choice": "marketing",
                "confidence": 1.0,
                "probabilities": { "marketing": 1.0 }
            }
        },
        "usage": {}
    }))
    .unwrap();

    let error = response.choice_as::<Department>("department").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("department"), "{message}");
    assert!(message.contains("marketing"), "{message}");
}

#[test]
fn a_missing_required_answer_lists_what_did_come_back() {
    let response: Response = serde_json::from_value(json!({
        "model": "jev-1.13.0",
        "answers": { "is_urgent": { "type": "noul", "noul": 0.5 } },
        "usage": {}
    }))
    .unwrap();

    let message = response.extract::<Triage>().unwrap_err().to_string();
    assert!(message.contains("refund"), "{message}");
    assert!(message.contains("is_urgent"), "{message}");
}

#[test]
fn hand_written_impls_interoperate_with_the_derived_ones() {
    use indexmap::IndexMap;
    use typesafe_api::UnknownOption;

    // A structured description is beyond what a doc comment can express, so
    // this option set is written out rather than derived.
    #[derive(Debug, PartialEq)]
    enum Topic {
        Policy,
        Status,
    }

    impl Options for Topic {
        fn criteria() -> IndexMap<String, Option<Entry>> {
            let mut map = IndexMap::new();
            map.insert(
                "return_policy".to_owned(),
                Some(Entry::fields([
                    (
                        "what",
                        Entry::text("Whether and how an item can be returned"),
                    ),
                    ("not_for", Entry::text("Progress of a return already sent")),
                ])),
            );
            map.insert("return_status".to_owned(), None);
            map
        }
        fn variants() -> &'static [&'static str] {
            &["return_policy", "return_status"]
        }
        fn option_name(&self) -> &'static str {
            match self {
                Self::Policy => "return_policy",
                Self::Status => "return_status",
            }
        }
        fn from_option(name: &str) -> Result<Self, UnknownOption> {
            match name {
                "return_policy" => Ok(Self::Policy),
                "return_status" => Ok(Self::Status),
                other => Err(UnknownOption {
                    answer: other.to_owned(),
                    expected: Self::variants().to_vec(),
                }),
            }
        }
    }

    #[derive(Evaluation)]
    struct Ask {
        /// Which returns topic is the customer asking about?
        #[allow(dead_code, reason = "only the generated questions() is under test")]
        topic: ChoiceOf<Topic>,
    }

    let Question::Choice(question) = &Ask::questions()["topic"] else {
        panic!("expected a choice")
    };
    assert!(matches!(
        question.criteria["return_policy"],
        Some(Entry::Object(_))
    ));
    assert_eq!(question.criteria["return_status"], None);
}
