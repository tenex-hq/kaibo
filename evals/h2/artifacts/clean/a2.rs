// Excerpt from crates/kaibo-core/src/event/tests.rs, copied verbatim.
use serde_json::Value;

use super::*;
use crate::frontmatter::Status;
use crate::query::{Facets, Hit, HitCensus};

fn caller() -> Caller {
    Caller {
        build_profile: "release",
        stdout_tty: false,
    }
}

fn hit(path: &str, score: f64) -> Hit {
    Hit {
        path: path.to_string(),
        title: "A Title".to_string(),
        status: Some(Status::Current),
        score,
        snippet: "snippet".to_string(),
        facets: Facets::default(),
    }
}

fn query_report(outcome: QueryOutcome, census: HitCensus) -> QueryReport {
    QueryReport {
        question: "how do we deploy".to_string(),
        include_drafts: false,
        self_heal: None,
        outcome,
        census,
    }
}

fn parse(event: &Event) -> Value {
    serde_json::from_str(&event.to_jsonl()).unwrap()
}

// --- the gap that is not a gap ----------------------------------------

#[test]
fn a_gap_whose_pages_were_all_withheld_as_drafts_is_readable_as_such() {
    let census = HitCensus {
        raw: 3,
        unaddressable: 0,
        withheld_unverified: 0,
        withheld_draft: 3,
        kept: 0,
    };
    let report = query_report(
        QueryOutcome::NoHits {
            moc: MocInventory::Domains(vec!["kaibo".to_string()]),
        },
        census,
    );

    let json = parse(&Event::from_query(&report, 1_700_000_000_000, 12, caller()));
    let attrs = &json["attributes"];

    assert_eq!(attrs["kaibo.outcome"], "gap");
    assert_eq!(attrs["kaibo.raw_hit_count"], 3);
    assert_eq!(attrs["kaibo.withheld_draft"], 3);
    assert_eq!(attrs["kaibo.hit_count"], 0);
}

#[test]
fn a_gap_with_an_empty_corpus_is_readable_as_a_different_situation() {
    let report = query_report(
        QueryOutcome::NoHits {
            moc: MocInventory::Domains(vec!["kaibo".to_string()]),
        },
        HitCensus::default(),
    );

    let json = parse(&Event::from_query(&report, 1_700_000_000_000, 12, caller()));
    let attrs = &json["attributes"];

    assert_eq!(attrs["kaibo.outcome"], "gap");
    assert_eq!(attrs["kaibo.raw_hit_count"], 0);
    assert_eq!(attrs["kaibo.withheld_draft"], 0);
}

// --- what the event must never claim ----------------------------------

#[test]
fn no_attribute_claims_the_answer_was_useful_or_that_activation_was_measured() {
    let report = query_report(
        QueryOutcome::Hits(vec![hit("kaibo/reference/a.md", 0.9)]),
        HitCensus::default(),
    );
    let json = parse(&Event::from_query(&report, 1_700_000_000_000, 5, caller()));
    let attrs = json["attributes"].as_object().unwrap();

    for forbidden in [
        "kaibo.useful",
        "kaibo.helpful",
        "kaibo.session_id",
        "kaibo.activation",
        "kaibo.activated",
    ] {
        assert!(
            !attrs.contains_key(forbidden),
            "event must not carry `{forbidden}`: it would imply a question the trail cannot answer"
        );
    }
}

#[test]
fn every_custom_attribute_is_namespaced_and_never_uses_the_reserved_otel_prefix() {
    let report = query_report(
        QueryOutcome::NoHits {
            moc: MocInventory::Domains(vec!["kaibo".to_string()]),
        },
        HitCensus::default(),
    );
    let json = parse(&Event::from_query(&report, 1_700_000_000_000, 5, caller()));

    for key in json["attributes"].as_object().unwrap().keys() {
        assert!(
            !key.starts_with("otel."),
            "`{key}` squats on the prefix the OTel spec reserves"
        );
        assert!(
            key.starts_with("kaibo.") || key.starts_with("process."),
            "`{key}` is neither a kaibo attribute nor a semconv one"
        );
    }
}
