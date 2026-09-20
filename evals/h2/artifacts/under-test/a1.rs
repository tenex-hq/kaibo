// Excerpt from crates/kaibo-core/src/trail/tests.rs, copied verbatim.
use std::fs;

use super::*;
use crate::event::Caller;
use crate::query::{HitCensus, MocInventory, QueryOutcome, QueryReport};

fn event(question: &str) -> Event {
    let report = QueryReport {
        question: question.to_string(),
        include_drafts: false,
        self_heal: None,
        outcome: QueryOutcome::NoHits {
            moc: MocInventory::Domains(vec![]),
        },
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
fn the_first_write_creates_the_workspace_directory_it_needs() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".kaibo").join("trail.jsonl");

    assert_eq!(append(&path, &event("first question")), TrailWrite::Written);

    assert!(path.is_file(), "the trail file was not created at {path:?}");
}

#[test]
fn a_second_invocation_appends_and_leaves_the_first_line_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("trail.jsonl");

    append(&path, &event("first question"));
    append(&path, &event("second question"));

    let lines: Vec<String> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();

    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], event("first question").to_jsonl());
    assert_eq!(lines[1], event("second question").to_jsonl());
}

#[test]
fn every_record_is_terminated_so_the_next_one_starts_a_new_line() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("trail.jsonl");

    append(&path, &event("only question"));

    let contents = fs::read_to_string(&path).unwrap();
    assert!(
        contents.ends_with('\n'),
        "an unterminated record would merge with the next invocation's line"
    );
}

#[test]
fn append_returns_trail_write_failed_when_the_parent_path_is_a_regular_file() {
    let tmp = tempfile::tempdir().unwrap();
    // A regular file where the trail expects a directory: `create_dir_all`
    // cannot fix this, and the situation is exactly what an unwritable
    // `~/.kaibo` looks like from inside the process.
    let blocker = tmp.path().join(".kaibo");
    fs::write(&blocker, "not a directory").unwrap();

    let outcome = append(&blocker.join("trail.jsonl"), &event("a question"));

    let TrailWrite::Failed { detail } = outcome else {
        panic!("writing under a regular file must not report success");
    };
    assert!(
        detail.contains("trail.jsonl"),
        "the failure has to name the path it could not write: {detail}"
    );
}

#[test]
fn a_failed_write_leaves_no_partial_file_behind_for_a_reader_to_trip_over() {
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join(".kaibo");
    fs::write(&blocker, "not a directory").unwrap();

    let trail = blocker.join("trail.jsonl");
    append(&trail, &event("a question"));

    assert!(
        !trail.is_file(),
        "a failed append must not leave a half-written record behind"
    );
}

// Set `KAIBO_TRAIL_SOAK` to run the long append loop. It only pays for itself
// when the record shape changes, so it stays inert unless the variable is set.
#[test]
fn a_long_run_of_appends_leaves_one_line_per_invocation_and_no_torn_record() {
    if std::env::var_os("KAIBO_TRAIL_SOAK").is_none() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("trail.jsonl");

    for index in 0..10_000 {
        append(&path, &event(&format!("question {index}")));
    }

    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents.lines().count(), 10_000);
    assert!(
        contents
            .lines()
            .all(|line| line.starts_with('{') && line.ends_with('}')),
        "a torn line would not be a complete JSON object"
    );
}
