use serde_json::json;

use super::*;
use crate::event::Caller;
use crate::query::{Facets, Hit, HitCensus, QueryOutcome, QueryReport};

fn event() -> Event {
    let report = QueryReport {
        question: "how do we deploy".to_string(),
        include_drafts: false,
        self_heal: None,
        outcome: QueryOutcome::Hits(vec![Hit {
            path: "docs/reference/good.md".to_string(),
            title: "Good Page".to_string(),
            status: Some(crate::frontmatter::Status::Current),
            score: 0.91,
            snippet: "snippet".to_string(),
            facets: Facets::default(),
        }]),
        census: HitCensus::default(),
    };
    Event::from_query(
        &report,
        1_700_000_000_000,
        7,
        Caller {
            build_profile: "release",
            stdout_tty: false,
        },
    )
}

#[test]
fn the_two_sinks_describe_the_same_invocation() {
    // The file and the collector must never drift: one attribute map feeds
    // both, and this is what says so.
    let event = event();
    let from_file: serde_json::Value = serde_json::from_str(&event.to_jsonl()).unwrap();

    let exported: Vec<String> = event.attribute_map().keys().cloned().collect();
    let written: Vec<String> = from_file["attributes"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();

    assert_eq!(exported, written);
    assert!(
        exported.contains(&"kaibo.subject".to_string()),
        "sanity: the comparison would pass on two empty lists too"
    );
}

#[test]
fn a_score_crosses_as_a_double_and_a_count_as_an_integer() {
    // JSON has one number type and OTLP has two. A hit count arriving as
    // 2.0 makes every downstream aggregation awkward.
    assert_eq!(any_value(json!(0.91)), Some(AnyValue::Double(0.91)));
    assert_eq!(any_value(json!(3)), Some(AnyValue::Int(3)));
}

#[test]
fn a_domain_list_crosses_as_a_list_rather_than_as_one_joined_string() {
    let Some(AnyValue::ListAny(values)) = any_value(json!(["kaibo", "observability"])) else {
        panic!("a JSON array must become an OTLP list");
    };
    assert_eq!(
        *values,
        vec![
            AnyValue::String("kaibo".into()),
            AnyValue::String("observability".into())
        ]
    );
}

#[test]
fn an_absent_attribute_stays_absent_rather_than_arriving_empty() {
    assert_eq!(any_value(json!(null)), None);
}

#[test]
fn a_boolean_stays_a_boolean_so_a_filter_on_it_can_be_written() {
    assert_eq!(any_value(json!(false)), Some(AnyValue::Boolean(false)));
}

#[test]
fn a_timestamp_beyond_what_milliseconds_can_hold_saturates_instead_of_wrapping() {
    // One millisecond past what a `u64` holds. `u128::MAX` is the wrong
    // probe: truncating all-ones gives all-ones, so a wrapping cast passes
    // it. This value truncates to zero, putting the event at the epoch,
    // which reads as a real timestamp and is the worse failure.
    let just_over = u128::from(u64::MAX) + 1;
    assert_eq!(duration_of(just_over), Duration::from_millis(u64::MAX));
    assert_eq!(
        duration_of(1_700_000_000_000),
        Duration::from_millis(1_700_000_000_000)
    );
}
