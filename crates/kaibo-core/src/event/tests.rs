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

fn hit(path: &str, relevance: f64) -> Hit {
    Hit {
        path: path.to_string(),
        title: "A Title".to_string(),
        status: Some(Status::Current),
        relevance,
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
        withheld_low_relevance: 0,
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

// --- outcome and exit code stay in step -------------------------------

#[test]
fn a_gap_records_the_exit_three_signal_and_a_hit_records_success() {
    let gap = query_report(
        QueryOutcome::NoHits {
            moc: MocInventory::Unavailable {
                detail: "missing".to_string(),
            },
        },
        HitCensus::default(),
    );
    let gap_json = parse(&Event::from_query(&gap, 1, 1, caller()));
    assert_eq!(gap_json["attributes"]["process.exit.code"], 3);
    assert_eq!(gap_json["attributes"]["kaibo.outcome"], "gap");

    let found = query_report(
        QueryOutcome::Hits(vec![hit("kaibo/reference/a.md", 0.9)]),
        HitCensus::default(),
    );
    let found_json = parse(&Event::from_query(&found, 1, 1, caller()));
    assert_eq!(found_json["attributes"]["process.exit.code"], 0);
    assert_eq!(found_json["attributes"]["kaibo.outcome"], "hit");
}

#[test]
fn a_failure_is_an_error_outcome_and_never_a_gap() {
    let report = query_report(
        QueryOutcome::QueryFailed {
            detail: "qmd exploded".to_string(),
        },
        HitCensus::default(),
    );
    let json = parse(&Event::from_query(&report, 1, 1, caller()));

    assert_eq!(json["attributes"]["kaibo.outcome"], "error");
    assert_ne!(json["attributes"]["process.exit.code"], 3);
}

// --- the top hit -------------------------------------------------------

#[test]
fn the_top_hit_recorded_is_the_first_of_the_ordered_list() {
    let report = query_report(
        QueryOutcome::Hits(vec![
            hit("kaibo/reference/first.md", 0.91),
            hit("kaibo/reference/second.md", 0.42),
        ]),
        HitCensus::default(),
    );
    let json = parse(&Event::from_query(&report, 1, 1, caller()));

    assert_eq!(
        json["attributes"]["kaibo.top_hit_path"],
        "kaibo/reference/first.md"
    );
    assert_eq!(json["attributes"]["kaibo.top_hit_score"], 0.91);
    assert_eq!(json["attributes"]["kaibo.hit_count"], 2);
}

// --- query has no domain, doctrine does -------------------------------

#[test]
fn a_query_event_carries_no_domain_because_query_searches_corpus_wide() {
    let report = query_report(
        QueryOutcome::Hits(vec![hit("kaibo/reference/a.md", 0.9)]),
        HitCensus::default(),
    );
    let json = parse(&Event::from_query(&report, 1, 1, caller()));

    assert!(
        json["attributes"]
            .as_object()
            .unwrap()
            .get("kaibo.domain")
            .is_none()
    );
}

// --- census only where a census exists --------------------------------

#[test]
fn a_doctrine_event_omits_the_census_rather_than_reporting_zeroes_it_never_counted() {
    let report = DoctrineReport {
        domain: "observability".to_string(),
        self_heal: None,
        outcome: DoctrineOutcome::UnknownDomain {
            available_domains: vec!["kaibo".to_string()],
        },
    };
    let json = parse(&Event::from_doctrine(&report, 1, 7, caller()));
    let attrs = json["attributes"].as_object().unwrap();

    assert_eq!(attrs["kaibo.domain"], "observability");
    assert_eq!(attrs["kaibo.outcome"], "gap");
    assert_eq!(attrs["kaibo.moc_domains"][0], "kaibo");
    for absent in [
        "kaibo.raw_hit_count",
        "kaibo.withheld_draft",
        "kaibo.withheld_unverified",
        "kaibo.top_hit_score",
    ] {
        assert!(
            !attrs.contains_key(absent),
            "`{absent}` has no meaning for doctrine, which never calls qmd"
        );
    }
}

// --- caller facts ------------------------------------------------------

#[test]
fn the_caller_is_described_by_observed_facts_and_carries_no_declared_identity() {
    let report = query_report(
        QueryOutcome::Hits(vec![hit("kaibo/reference/a.md", 0.9)]),
        HitCensus::default(),
    );
    let json = parse(&Event::from_query(&report, 1, 1, caller()));
    let attrs = json["attributes"].as_object().unwrap();

    assert_eq!(attrs["kaibo.build_profile"], "release");
    assert_eq!(attrs["kaibo.stdout_tty"], false);
    assert!(
        !attrs.contains_key("kaibo.caller"),
        "a declared caller label is the contamination ADR 0015 rejects"
    );
}

#[test]
fn a_debug_build_is_observable_without_anyone_declaring_it() {
    let observed = Caller::observed(true);
    let expected = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    assert_eq!(observed.build_profile, expected);
    assert!(observed.stdout_tty);
}

// --- one line, and it parses ------------------------------------------

#[test]
fn an_event_serialises_to_exactly_one_line_so_the_file_stays_greppable() {
    let report = query_report(
        QueryOutcome::Hits(vec![hit("kaibo/reference/a.md", 0.9)]),
        HitCensus::default(),
    );
    let line = Event::from_query(&report, 1_700_000_000_000, 5, caller()).to_jsonl();

    assert!(!line.contains('\n'));
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["event_name"],
        "kaibo.query"
    );
}

#[test]
fn a_question_with_a_newline_in_it_cannot_break_the_one_event_per_line_contract() {
    let report = query_report(
        QueryOutcome::NoHits {
            moc: MocInventory::Domains(vec![]),
        },
        HitCensus::default(),
    );
    let mut report = report;
    report.question = "first line\nsecond line".to_string();

    let line = Event::from_query(&report, 1, 1, caller()).to_jsonl();

    assert!(
        !line.contains('\n'),
        "a newline in the question must be escaped, not emitted raw"
    );
    let parsed: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        parsed["attributes"]["kaibo.subject"],
        "first line\nsecond line"
    );
}

// --- self-heal explains latency ---------------------------------------

#[test]
fn a_fresh_corpus_records_no_self_heal_because_no_git_call_happened() {
    let mut report = query_report(
        QueryOutcome::Hits(vec![hit("kaibo/reference/a.md", 0.9)]),
        HitCensus::default(),
    );
    report.self_heal = Some(SyncOutcome::SkippedFresh);

    let json = parse(&Event::from_query(&report, 1, 1, caller()));
    assert_eq!(json["attributes"]["kaibo.self_heal"], "none");
}
