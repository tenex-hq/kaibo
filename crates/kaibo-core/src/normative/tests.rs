use super::*;
use crate::frontmatter;

/// Every test starts from a real page, parsed by the real frontmatter
/// parser, so nothing here can pass on a YAML shape the corpus cannot
/// actually produce.
fn parse_page(frontmatter_yaml: &str, body: &str) -> Result<Option<Standard>, NormativeError> {
    let doc = frontmatter::parse(&format!("---\n{frontmatter_yaml}\n---\n{body}\n")).unwrap();
    parse(&doc.frontmatter, &doc.body)
}

const BINDING_KEYS: &str = "\
type: reference
title: A standard
status: current
binding: true
severity: must
applies_to:
  actions: [file-edit]";

// --- a page that declares nothing normative -----------------------------

#[test]
fn an_ordinary_reference_page_is_not_a_standard() {
    let parsed = parse_page(
        "type: reference\ntitle: A page\nstatus: current",
        "Prose, and a fenced block that is not ours:\n\n```json\n{\"a\": 1}\n```\n",
    );
    assert_eq!(parsed.unwrap(), None);
}

#[test]
fn binding_false_on_its_own_is_the_default_said_out_loud_not_an_error() {
    let parsed = parse_page("type: reference\nbinding: false", "Prose.\n");
    assert_eq!(parsed.unwrap(), None);
}

// --- the complete, well-formed standard ---------------------------------

#[test]
fn a_binding_page_carries_severity_the_actions_it_applies_to_and_its_checks() {
    let parsed = parse_page(
        "\
type: reference
title: No print in library code
status: current
binding: true
severity: must
applies_to:
  actions: [file-edit, commit-message]
  tags: [workload-repo, python]",
        "\
Library code writes to a logger, never to stdout.

```json kaibo-checks
[
  { \"id\": \"no-print\", \"kind\": \"forbid_regex\", \"pattern\": \"^\\\\s*print\\\\(\" }
]
```
",
    );

    let standard = parsed.unwrap().expect("the page is binding");

    assert_eq!(standard.severity, Severity::Must);
    assert_eq!(
        standard.applies_to.actions,
        vec![ActionKind::FileEdit, ActionKind::CommitMessage]
    );
    assert_eq!(
        standard.applies_to.tags,
        vec!["workload-repo".to_string(), "python".to_string()]
    );
    assert_eq!(standard.checks.len(), 1);
    assert_eq!(standard.checks[0].id, "no-print");
    assert!(matches!(
        standard.checks[0].kind,
        CheckKind::ForbidRegex { .. }
    ));
}

#[test]
fn a_binding_page_needs_no_checks_at_all() {
    // A standard whose rule no regex can express is still binding, still
    // carries a severity, and still belongs in a contract as prose. Making
    // `checks` mandatory would silently exclude every standard that is not
    // regex-shaped, which is most of them.
    let parsed = parse_page(BINDING_KEYS, "Prose only.\n");

    let standard = parsed.unwrap().expect("the page is binding");

    assert!(standard.checks.is_empty());
}

#[test]
fn applies_to_needs_no_tags() {
    let parsed = parse_page(BINDING_KEYS, "Prose.\n");
    assert!(parsed.unwrap().unwrap().applies_to.tags.is_empty());
}

#[test]
fn severity_should_is_the_other_half_of_the_vocabulary() {
    let parsed = parse_page(
        &BINDING_KEYS.replace("severity: must", "severity: should"),
        "P\n",
    );
    assert_eq!(parsed.unwrap().unwrap().severity, Severity::Should);
}

#[test]
fn every_action_kind_in_the_closed_vocabulary_parses() {
    let parsed = parse_page(
        &BINDING_KEYS.replace(
            "actions: [file-edit]",
            "actions: [file-edit, commit-message, shell-command, chat, deploy, adr]",
        ),
        "Prose.\n",
    );

    assert_eq!(
        parsed.unwrap().unwrap().applies_to.actions,
        vec![
            ActionKind::FileEdit,
            ActionKind::CommitMessage,
            ActionKind::ShellCommand,
            ActionKind::Chat,
            ActionKind::Deploy,
            ActionKind::Adr,
        ]
    );
}

// --- all or nothing -----------------------------------------------------

#[test]
fn a_severity_without_binding_true_is_a_loud_error_not_a_half_loaded_page() {
    let parsed = parse_page("type: reference\nseverity: must", "Prose.\n");

    let err = parsed.unwrap_err();

    assert!(matches!(
        err,
        NormativeError::NormativeKeyWithoutBinding { .. }
    ));
    assert!(err.to_string().contains("severity"));
    assert!(err.to_string().contains("binding: true"));
}

#[test]
fn a_checks_block_without_binding_true_is_a_loud_error() {
    let parsed = parse_page(
        "type: reference",
        "```json kaibo-checks\n[{\"id\": \"x\", \"kind\": \"forbid_regex\", \"pattern\": \"a\"}]\n```\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(
        err,
        NormativeError::NormativeKeyWithoutBinding { .. }
    ));
    assert!(err.to_string().contains("kaibo-checks"));
}

#[test]
fn a_binding_page_without_a_severity_is_a_loud_error() {
    let parsed = parse_page(
        "type: reference\nbinding: true\napplies_to:\n  actions: [file-edit]",
        "Prose.\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(
        err,
        NormativeError::MissingNormativeKey { key: "severity" }
    ));
}

#[test]
fn a_binding_page_without_applies_to_is_a_loud_error() {
    let parsed = parse_page("type: reference\nbinding: true\nseverity: must", "Prose.\n");

    let err = parsed.unwrap_err();

    assert!(matches!(
        err,
        NormativeError::MissingNormativeKey { key: "applies_to" }
    ));
}

#[test]
fn a_binding_page_naming_no_action_can_never_be_selected_so_it_is_an_error() {
    // `applies_to` keys on the action the caller is about to take. Tags
    // filter; they do not address. A standard with tags and no action is
    // unreachable by every caller, which is worse than one that fails to
    // parse.
    let parsed = parse_page(
        "type: reference\nbinding: true\nseverity: must\napplies_to:\n  tags: [python]",
        "Prose.\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::NoActions));
}

#[test]
fn a_checks_fence_nested_inside_a_wider_fence_is_an_example_not_a_block() {
    // A page that documents the schema quotes a checks block inside a wider
    // fence. Grepping for the marker would turn every such page into a
    // standard, and a broken one at that.
    let parsed = parse_page(
        "type: reference\ntitle: How to write a standard\nstatus: current",
        "\
Write it like this:

````markdown
```json kaibo-checks
[{ \"id\": \"example\", \"kind\": \"forbid_regex\", \"pattern\": \"TODO\" }]
```
````
",
    );

    assert_eq!(parsed.unwrap(), None);
}

#[test]
fn a_binding_pages_own_block_is_still_found_when_the_page_also_quotes_one() {
    let parsed = parse_page(
        BINDING_KEYS,
        "\
````markdown
```json kaibo-checks
[{ \"id\": \"quoted\", \"kind\": \"forbid_regex\", \"pattern\": \"TODO\" }]
```
````

```json kaibo-checks
[{ \"id\": \"real\", \"kind\": \"forbid_regex\", \"pattern\": \"TODO\" }]
```
",
    );

    let checks = parsed.unwrap().unwrap().checks;

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].id, "real");
}

// --- the vocabularies are closed ----------------------------------------

#[test]
fn a_severity_outside_the_vocabulary_is_an_error_naming_both_allowed_values() {
    let parsed = parse_page(
        &BINDING_KEYS.replace("severity: must", "severity: high"),
        "P\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::UnknownSeverity(_)));
    let message = err.to_string();
    assert!(message.contains("high"));
    assert!(message.contains("must"));
    assert!(message.contains("should"));
}

#[test]
fn an_action_kind_outside_the_vocabulary_is_an_error_naming_the_whole_vocabulary() {
    let parsed = parse_page(
        &BINDING_KEYS.replace(
            "actions: [file-edit]",
            "actions: [file-edit, merge-request]",
        ),
        "Prose.\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::UnknownAction(_)));
    let message = err.to_string();
    assert!(message.contains("merge-request"));
    for known in [
        "file-edit",
        "commit-message",
        "shell-command",
        "chat",
        "deploy",
        "adr",
    ] {
        assert!(message.contains(known), "{message:?} should name {known}");
    }
}

#[test]
fn binding_is_a_boolean_and_a_string_saying_required_does_not_bind() {
    // The whole schema keys off `binding: true` being a YAML boolean. A
    // page saying `binding: required` looks binding to a human and is not
    // one to any reader, which is exactly the silent half-load this schema
    // exists to prevent.
    let parsed = parse_page("type: reference\nbinding: required", "Prose.\n");

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::BindingNotABoolean(_)));
    assert!(err.to_string().contains("required"));
}

// --- the checks block ---------------------------------------------------

#[test]
fn all_four_decidable_check_kinds_parse() {
    let parsed = parse_page(
        BINDING_KEYS,
        "\
```json kaibo-checks
[
  { \"id\": \"a\", \"kind\": \"forbid_regex\",  \"pattern\": \"TODO\" },
  { \"id\": \"b\", \"kind\": \"require_regex\", \"pattern\": \"SPDX\" },
  { \"id\": \"c\", \"kind\": \"require_if_present\", \"if_present\": \"def test_\", \"require\": \"assert \" },
  { \"id\": \"d\", \"kind\": \"forbid_path\",   \"pattern\": \"^vendor/\" }
]
```
",
    );

    let checks = parsed.unwrap().unwrap().checks;

    assert_eq!(
        checks.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["a", "b", "c", "d"]
    );
    assert!(matches!(checks[0].kind, CheckKind::ForbidRegex { .. }));
    assert!(matches!(checks[1].kind, CheckKind::RequireRegex { .. }));
    assert!(matches!(checks[2].kind, CheckKind::RequireIfPresent { .. }));
    assert!(matches!(checks[3].kind, CheckKind::ForbidPath { .. }));
}

#[test]
fn a_judgment_check_is_not_in_the_vocabulary_and_says_so_rather_than_being_dropped() {
    // Decidable-only, on H2's evidence. An author who writes a judgment
    // item must be told it does nothing, not have it silently discarded
    // into a contract that then under-reports what it checked.
    let parsed = parse_page(
        BINDING_KEYS,
        "```json kaibo-checks\n[{\"id\": \"x\", \"kind\": \"judgment\", \"criterion\": \"is it tasteful\"}]\n```\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::MalformedChecks { .. }));
    assert!(err.to_string().contains("judgment"));
}

#[test]
fn a_checks_block_that_is_not_json_is_a_loud_error_not_an_empty_check_list() {
    let parsed = parse_page(
        BINDING_KEYS,
        "```json kaibo-checks\n[{ id: no-quotes }]\n```\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::MalformedChecks { .. }));
}

#[test]
fn a_checks_block_that_is_a_json_object_rather_than_an_array_is_an_error() {
    let parsed = parse_page(
        BINDING_KEYS,
        "```json kaibo-checks\n{\"checks\": []}\n```\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::MalformedChecks { .. }));
}

#[test]
fn a_check_missing_the_field_its_kind_needs_is_an_error_naming_the_field() {
    let parsed = parse_page(
        BINDING_KEYS,
        "```json kaibo-checks\n[{\"id\": \"a\", \"kind\": \"require_if_present\", \"if_present\": \"x\"}]\n```\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::MalformedChecks { .. }));
    assert!(err.to_string().contains("require"));
}

#[test]
fn a_pattern_that_is_not_a_valid_regex_fails_here_rather_than_at_judgment_time() {
    let parsed = parse_page(
        BINDING_KEYS,
        "```json kaibo-checks\n[{\"id\": \"a\", \"kind\": \"forbid_regex\", \"pattern\": \"[unclosed\"}]\n```\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::InvalidPattern { .. }));
    let message = err.to_string();
    assert!(message.contains("[unclosed"));
    assert!(message.contains('a'));
}

#[test]
fn two_checks_sharing_an_id_are_an_error_because_a_verdict_is_keyed_on_it() {
    let parsed = parse_page(
        BINDING_KEYS,
        "```json kaibo-checks\n[{\"id\": \"a\", \"kind\": \"forbid_regex\", \"pattern\": \"x\"},\n {\"id\": \"a\", \"kind\": \"require_regex\", \"pattern\": \"y\"}]\n```\n",
    );

    let err = parsed.unwrap_err();

    assert!(matches!(err, NormativeError::DuplicateCheckId(_)));
    assert!(err.to_string().contains('a'));
}

#[test]
fn an_empty_check_id_is_an_error() {
    let parsed = parse_page(
        BINDING_KEYS,
        "```json kaibo-checks\n[{\"id\": \"\", \"kind\": \"forbid_regex\", \"pattern\": \"x\"}]\n```\n",
    );

    assert!(matches!(parsed.unwrap_err(), NormativeError::EmptyCheckId));
}

#[test]
fn two_checks_blocks_on_one_page_are_an_error_rather_than_a_silent_pick() {
    // Taking the first would half-load the page; concatenating them would
    // make the order of two fences load-bearing. Neither is a schema.
    let parsed = parse_page(
        BINDING_KEYS,
        "\
```json kaibo-checks
[{\"id\": \"a\", \"kind\": \"forbid_regex\", \"pattern\": \"x\"}]
```

More prose.

```json kaibo-checks
[{\"id\": \"b\", \"kind\": \"forbid_regex\", \"pattern\": \"y\"}]
```
",
    );

    assert!(matches!(
        parsed.unwrap_err(),
        NormativeError::MultipleChecksBlocks
    ));
}

#[test]
fn an_unclosed_checks_fence_is_an_error_not_a_block_running_to_end_of_file() {
    let parsed = parse_page(
        BINDING_KEYS,
        "```json kaibo-checks\n[{\"id\": \"a\", \"kind\": \"forbid_regex\", \"pattern\": \"x\"}]\n",
    );

    assert!(matches!(
        parsed.unwrap_err(),
        NormativeError::UnterminatedChecksBlock
    ));
}

#[test]
fn an_empty_checks_array_is_the_same_as_no_block() {
    let parsed = parse_page(BINDING_KEYS, "```json kaibo-checks\n[]\n```\n");
    assert!(parsed.unwrap().unwrap().checks.is_empty());
}

#[test]
fn a_shorter_fence_does_not_close_a_wider_checks_block() {
    // CommonMark: a closing fence is at least as long as the one that
    // opened it. Without that, a page whose checks contain a fenced example
    // would end at the example instead of at its own closing fence, and the
    // half it kept would still be valid JSON often enough to pass silently.
    let parsed = parse_page(
        BINDING_KEYS,
        "\
````json kaibo-checks
[{ \"id\": \"a\", \"kind\": \"forbid_regex\", \"pattern\": \"x\" }]
```
",
    );

    assert!(matches!(
        parsed.unwrap_err(),
        NormativeError::UnterminatedChecksBlock
    ));
}

#[test]
fn a_tilde_fence_does_not_close_a_backtick_checks_block() {
    let parsed = parse_page(
        BINDING_KEYS,
        "\
```json kaibo-checks
[{ \"id\": \"a\", \"kind\": \"forbid_regex\", \"pattern\": \"x\" }]
~~~
",
    );

    assert!(matches!(
        parsed.unwrap_err(),
        NormativeError::UnterminatedChecksBlock
    ));
}

#[test]
fn a_run_of_three_ordinary_characters_is_not_a_fence() {
    // Only backticks and tildes fence. A heading, a setext underline or a
    // horizontal rule above the block must not open one, or the page's real
    // fence lands inside a block that was never opened.
    let parsed = parse_page(
        BINDING_KEYS,
        "\
### How this is checked

---

```json kaibo-checks
[{ \"id\": \"a\", \"kind\": \"forbid_regex\", \"pattern\": \"x\" }]
```
",
    );

    let checks = parsed.unwrap().unwrap().checks;

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].id, "a");
}

// --- corpus content in an error message ---------------------------------

#[test]
fn a_control_character_in_a_rejected_value_never_reaches_the_error_message() {
    // An error message is output, and output carrying corpus bytes gets the
    // same stripping every other surface applies: a standard's author does
    // not get to move the reader's cursor.
    let parsed = parse_page(
        "type: reference\nbinding: true\nseverity: \"hi\\rgh\"\napplies_to:\n  actions: [file-edit]",
        "P\n",
    );

    let message = parsed.unwrap_err().to_string();

    assert!(!message.contains('\r'), "{message:?}");
}

#[test]
fn a_control_character_in_a_tag_never_reaches_the_parsed_standard() {
    let parsed = parse_page(
        &BINDING_KEYS.replace(
            "actions: [file-edit]",
            "actions: [file-edit]\n  tags: [\"py\\rthon\"]",
        ),
        "Prose.\n",
    );

    assert_eq!(parsed.unwrap().unwrap().applies_to.tags, vec!["python"]);
}

// --- the exit code contract ---------------------------------------------

#[test]
fn a_schema_error_is_bad_input_not_an_internal_failure() {
    use crate::error::{ExitCode, ExitCoded};

    let parsed = parse_page("type: reference\nseverity: must", "P\n");

    assert_eq!(parsed.unwrap_err().exit_code(), ExitCode::Usage);
}

// --- the schema and the page that documents it ---------------------------

/// The authoritative schema doc, read at compile time: a drift test that
/// went to the filesystem would pass on a machine where the file moved.
const CONVENTIONS: &str = include_str!("../../../../template/CONVENTIONS.md");

#[test]
fn every_action_kind_the_code_knows_is_documented() {
    for action in ActionKind::ALL {
        assert!(
            CONVENTIONS.contains(&format!("`{}`", action.as_str())),
            "template/CONVENTIONS.md never mentions the action kind {:?}",
            action.as_str()
        );
    }
}

#[test]
fn every_check_kind_the_docs_show_is_one_the_code_accepts() {
    // Swept out of the doc rather than listed here: a check kind added to
    // the doc and not to the vocabulary is exactly the drift a list of
    // known-good names would keep passing through.
    let documented: Vec<&str> = CONVENTIONS
        .match_indices("\"kind\": \"")
        .map(|(at, marker)| {
            let rest = &CONVENTIONS[at + marker.len()..];
            &rest[..rest.find('"').expect("a quoted kind closes its quote")]
        })
        .collect();

    assert!(
        documented.len() >= 4,
        "the doc should show every kind; found {documented:?}"
    );

    for kind in documented {
        let json = format!(
            "[{{\"id\": \"a\", \"kind\": \"{kind}\", \"pattern\": \"x\", \"if_present\": \"x\", \"require\": \"x\"}}]"
        );
        let parsed = parse_page(
            BINDING_KEYS,
            &format!("```json kaibo-checks\n{json}\n```\n"),
        );
        assert!(
            parsed.is_ok(),
            "template/CONVENTIONS.md documents the check kind {kind:?}, which the schema rejects"
        );
    }
}

#[test]
fn every_severity_the_code_knows_is_documented() {
    for severity in Severity::ALL {
        assert!(
            CONVENTIONS.contains(&format!("`{}`", severity.as_str())),
            "template/CONVENTIONS.md never mentions the severity {:?}",
            severity.as_str()
        );
    }
}
