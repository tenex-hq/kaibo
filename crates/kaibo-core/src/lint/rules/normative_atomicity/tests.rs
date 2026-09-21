use super::*;

/// Frontmatter for a page that really is a binding standard, so
/// `normative::parse` returns `Some` and the rule engages at all.
const BINDING: &str = "type: reference
title: Library code logs, it does not print
tags: [python]
status: current
updated: 2026-09-21
binding: true
severity: must
applies_to:
  actions: [file-edit]";

const NOT_BINDING: &str = "type: reference
title: How the logger is configured
tags: [python]
status: current
updated: 2026-09-21";

fn page(frontmatter_yaml: &str, body: &str) -> LintedFile {
    let doc = crate::frontmatter::parse(&format!("---\n{frontmatter_yaml}\n---\n{body}\n"))
        .expect("fixture frontmatter parses");
    LintedFile {
        repo_relative_path: "python/reference/logging.md".to_string(),
        frontmatter: Ok(doc.frontmatter),
        body: doc.body,
    }
}

fn rule() -> NormativeAtomicityRule {
    NormativeAtomicityRule::new(1)
}

#[test]
fn the_rules_id_is_normative_atomicity() {
    assert_eq!(rule().id(), "normative-atomicity");
}

#[test]
fn a_page_that_enumerates_rules_is_reported_as_a_suggestion_not_a_gate() {
    let file = page(
        BINDING,
        "- Library code must not print.\n- Handlers should be set by the application.\n- A log line never contains a secret.",
    );
    let violations = rule().check(&file);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].severity, Severity::Heuristic);
}

#[test]
fn a_page_stating_one_claim_over_several_sentences_is_left_alone() {
    let file = page(
        BINDING,
        "Library code must not print. It has no idea where its output goes, \
         and the application that imported it does. So it logs, and the \
         application decides where that goes.",
    );
    assert_eq!(rule().check(&file), Vec::new());
}

#[test]
fn a_page_that_is_not_a_binding_standard_may_state_as_many_claims_as_it_likes() {
    let file = page(
        NOT_BINDING,
        "- You must configure a handler.\n- You should not print.\n- Never log a secret.",
    );
    assert_eq!(rule().check(&file), Vec::new());
}

#[test]
fn a_page_whose_frontmatter_did_not_parse_is_not_this_rules_business() {
    let file = LintedFile {
        repo_relative_path: "python/reference/logging.md".to_string(),
        frontmatter: Err("mapping values are not allowed here".to_string()),
        body: "- Must one.\n- Must two.\n- Must three.".to_string(),
    };
    assert_eq!(rule().check(&file), Vec::new());
}

#[test]
fn the_message_names_both_the_count_and_the_ceiling_it_passed() {
    let file = page(
        BINDING,
        "- Library code must not print.\n- A handler is always the application's job.\n- Secrets are forbidden in a log line.",
    );
    let violations = rule().check(&file);
    assert!(
        violations[0].message.contains("states 3 normative claims"),
        "message did not name the count: {}",
        violations[0].message
    );
    assert!(
        violations[0].message.contains("ceiling is 1"),
        "message did not name the ceiling: {}",
        violations[0].message
    );
}

#[test]
fn the_ceiling_is_the_configured_one_not_a_hardcoded_one() {
    let body = "- Library code must not print.\n- A handler is always the application's job.\n- Secrets are forbidden in a log line.";
    assert_eq!(
        NormativeAtomicityRule::new(3).check(&page(BINDING, body)),
        Vec::new()
    );
    assert_eq!(
        NormativeAtomicityRule::new(2)
            .check(&page(BINDING, body))
            .len(),
        1
    );
}

#[test]
fn two_paragraphs_each_stating_a_claim_are_two_claims() {
    let file = page(
        BINDING,
        "Library code must not print.\n\nA handler is always the application's job.",
    );
    assert_eq!(rule().check(&file).len(), 1);
}

#[test]
fn a_heading_does_not_count_as_a_claim_of_its_own() {
    // The heading names the claim stated below it. Counting both would
    // report every well-structured page that uses one.
    let file = page(BINDING, "## Never print\n\nLibrary code must not print.");
    assert_eq!(rule().check(&file), Vec::new());
}

#[test]
fn claim_words_inside_a_fenced_block_are_not_claims() {
    let file = page(
        BINDING,
        "Library code must not print.\n\n```json kaibo-checks\n[\n  { \"id\": \"must-not-print\", \"kind\": \"forbid_regex\", \"pattern\": \"^\\\\s*print\\\\(\" }\n]\n```\n",
    );
    assert_eq!(rule().check(&file), Vec::new());
}

#[test]
fn a_shorter_fence_does_not_close_a_wider_one() {
    // Four backticks opened it, so the three inside are content. A scanner
    // that closed on the first fence would let the tail count as prose.
    let file = page(
        BINDING,
        "Library code must not print.\n\n````\n```\nYou should always do this.\nYou must never do that.\n```\n````\n",
    );
    assert_eq!(rule().check(&file), Vec::new());
}

#[test]
fn a_backtick_fence_does_not_close_a_tilde_fence() {
    let file = page(
        BINDING,
        "Library code must not print.\n\n~~~\n```\nYou should always do this.\nYou must never do that.\n~~~\n",
    );
    assert_eq!(rule().check(&file), Vec::new());
}

#[test]
fn a_claim_word_inside_a_longer_word_is_not_a_claim() {
    // "mustard" and "shallow" are not obligations.
    let file = page(
        BINDING,
        "Library code must not print.\n\nThe mustard jar is shallow.\n\nAlwayswhen is not a word.",
    );
    assert_eq!(rule().check(&file), Vec::new());
}

#[test]
fn a_claim_word_is_recognised_whatever_its_case() {
    let file = page(BINDING, "Never print.\n\nNEVER print to stderr either.");
    assert_eq!(rule().check(&file).len(), 1);
}

#[test]
fn a_numbered_list_splits_into_one_claim_per_item() {
    let file = page(
        BINDING,
        "1. Library code must not print.\n2. A handler is always the application's job.",
    );
    assert_eq!(rule().check(&file).len(), 1);
}

#[test]
fn a_hyphen_that_opens_a_word_does_not_open_a_list_item() {
    // "-ish" is prose. Splitting on it would inflate the count.
    assert!(!starts_a_list_item("-ish, at the edges, you must decide"));
    assert!(starts_a_list_item("- you must decide"));
}

#[test]
fn a_digit_not_followed_by_a_list_marker_does_not_open_a_list_item() {
    assert!(!starts_a_list_item("2026 was the year you must remember"));
    assert!(starts_a_list_item("2. you must remember"));
    assert!(starts_a_list_item("2) you must remember"));
}

#[test]
fn a_page_with_no_claim_words_at_all_is_not_reported() {
    let file = page(BINDING, "This page explains the logging setup.");
    assert_eq!(rule().check(&file), Vec::new());
}

#[test]
fn every_claim_marker_is_recognised_on_its_own() {
    // The list is the rule's whole sensitivity. A marker silently dropped
    // from it would make a page stop being counted with nothing failing.
    for marker in CLAIM_MARKERS {
        assert!(
            states_a_claim(&format!("Library code {marker} print to stdout.")),
            "{marker} did not read as a claim"
        );
    }
}

// The three closing conditions of the fence scanner, each held by a page
// whose count changes when that condition goes. A test that only asserts
// "no violation" cannot tell a scanner that closed correctly from one that
// swallowed the rest of the file, so each of these needs prose on the far
// side of a fence that does close.

#[test]
fn prose_after_a_closed_fence_is_counted_again() {
    // A scanner that never closes swallows the tail, and every one of the
    // fence tests above would still pass while doing it.
    let file = page(
        BINDING,
        "Library code must not print.\n\n```\nexample\n```\n\nA handler is always the application's job.",
    );
    assert_eq!(rule().check(&file).len(), 1);
}

#[test]
fn a_tilde_fence_hides_its_contents_the_same_as_a_backtick_fence() {
    // Tilde fences are the half of the CommonMark rule a scanner is most
    // likely to lose: nothing in a backtick test notices they are gone.
    let file = page(
        BINDING,
        "Library code must not print.\n\n~~~\nYou should always do this.\n~~~\n",
    );
    assert_eq!(rule().check(&file), Vec::new());
}
