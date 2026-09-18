//! End-to-end tests: the real, compiled `kaibo` binary, run as a subprocess
//! against stub `git`/`qmd` executables on `PATH` (see `support::Harness`).
//! No other test in this workspace exercises the binary itself - every
//! other test calls into `kaibo-core` directly.
//!
//! `ConfigBuilder` is `#[cfg(test)] pub(crate)` inside `kaibo-core` and not
//! visible here, so every test drives `Config::resolve()` the only way a
//! real invocation can: through `KAIBO_*` environment variables and a
//! scratch `HOME` (see `support::Harness::run`).
//!
//! Every verb is covered for: the process exit code on a healthy corpus,
//! that `--explain` runs nothing, and that `--json` parses. The hostile
//! corpus fixture (`support::write_hostile_corpus`) additionally proves
//! that a corpus-reading verb (`query`, `doctrine`, `lint`) admits only
//! the pages its own trust rules say it should, and that no verb's own
//! output lines or the commands it runs are altered by what the hostile
//! corpus contains.

mod support;

use support::Harness;

fn parse_json(stdout: &[u8]) -> serde_json::Value {
    serde_json::from_slice(stdout).unwrap_or_else(|err| {
        panic!(
            "stdout was not valid JSON ({err}): {:?}",
            String::from_utf8_lossy(stdout)
        )
    })
}

// --- status ---------------------------------------------------------------

#[test]
fn status_explain_runs_nothing_and_exits_success() {
    let harness = Harness::new();

    let output = harness.run(&["--explain", "status"], &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(harness.calls().is_empty(), "explain must not run anything");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        5,
        "expected exactly the 5 planned status commands, got: {stdout}"
    );
}

#[test]
fn status_reports_success_exit_code_on_a_healthy_corpus() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["status"], &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn status_json_output_parses_and_reports_the_exit_code() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["--json", "status"], &[]);

    let json = parse_json(&output.stdout);
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["clone"]["present"], true);
}

/// `status` never reads `_index.md` or any `reference/` page - its output
/// and the commands it runs must be exactly the same whether the corpus
/// content is the minimal fixture or the full hostile one.
#[test]
fn status_output_and_commands_are_unaffected_by_hostile_corpus_content() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    let minimal_output = harness.run(&["status"], &[]);
    let minimal_calls = harness.calls();
    harness.reset_calls();

    // Overwrite in place: same harness, same absolute clone path, so the
    // only thing that changed is what is inside the corpus.
    std::fs::remove_dir_all(harness.clone_dir().join("docs")).unwrap();
    support::write_hostile_corpus(&harness.clone_dir(), &harness.outside_dir());
    let hostile_output = harness.run(&["status"], &[]);
    let hostile_calls = harness.calls();

    assert_eq!(minimal_output.stdout, hostile_output.stdout);
    assert_eq!(minimal_calls, hostile_calls);
}

// --- sync -------------------------------------------------------------

#[test]
fn sync_explain_runs_nothing_and_exits_success() {
    let harness = Harness::new();

    let output = harness.run(&["--explain", "sync"], &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(harness.calls().is_empty(), "explain must not run anything");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        7,
        "expected exactly the 7 planned sync commands, got: {stdout}"
    );
}

#[test]
fn sync_reports_success_exit_code_on_a_healthy_corpus() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["sync"], &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sync_json_output_parses_and_reports_the_exit_code() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["--json", "sync"], &[]);

    let json = parse_json(&output.stdout);
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["outcome"]["state"], "completed");
}

/// A corpus that has never been synced (no `.git`) and no configured repo
/// must fail with the documented usage exit code, naming the fix.
#[test]
fn sync_reports_usage_exit_code_when_no_clone_and_no_repo_are_configured() {
    let harness = Harness::new();
    harness.forget_clone();

    let output = harness.run(&["sync"], &[]);

    assert_eq!(output.status.code(), Some(2));
    let stderr_and_stdout = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stderr_and_stdout.contains("KAIBO_REPO"));
}

/// `sync` never reads `_index.md` or any `reference/` page - its output and
/// the commands it runs must be exactly the same whether the corpus
/// content is the minimal fixture or the full hostile one.
#[test]
fn sync_output_and_commands_are_unaffected_by_hostile_corpus_content() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    let minimal_output = harness.run(&["sync"], &[]);
    let minimal_calls = harness.calls();
    harness.reset_calls();

    std::fs::remove_dir_all(harness.clone_dir().join("docs")).unwrap();
    support::write_hostile_corpus(&harness.clone_dir(), &harness.outside_dir());
    let hostile_output = harness.run(&["sync"], &[]);
    let hostile_calls = harness.calls();

    assert_eq!(minimal_output.stdout, hostile_output.stdout);
    assert_eq!(minimal_calls, hostile_calls);
}

// --- query --------------------------------------------------------------

#[test]
fn query_explain_runs_nothing_and_exits_success() {
    let harness = Harness::new();

    let output = harness.run(&["--explain", "query", "a question"], &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(harness.calls().is_empty(), "explain must not run anything");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        8,
        "expected exactly the 8 planned query commands, got: {stdout}"
    );
}

#[test]
fn query_reports_success_exit_code_on_a_healthy_corpus() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    let response =
        support::write_query_response(&harness.outside_dir(), support::MINIMAL_QUERY_RESPONSE);

    let output = harness.run(
        &["query", "a question about the fixture"],
        &[("KAIBO_TEST_QMD_QUERY_RESPONSE", response.to_str().unwrap())],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn query_json_output_parses_and_reports_the_exit_code() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    let response =
        support::write_query_response(&harness.outside_dir(), support::MINIMAL_QUERY_RESPONSE);

    let output = harness.run(
        &["--json", "query", "a question about the fixture"],
        &[("KAIBO_TEST_QMD_QUERY_RESPONSE", response.to_str().unwrap())],
    );

    let json = parse_json(&output.stdout);
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["outcome"]["state"], "hits");
}

/// The hostile corpus's `file` fields include a path-traversal hit, an
/// absolute-path hit, a real symlink escaping the clone, and a page with
/// malformed frontmatter around a `draft` status - none of those four may
/// appear in `query`'s hits. The five verified, `current`, contained pages
/// must appear, in ranked order, each with its title and status stripped
/// of the control characters it was seeded with, and each fenced exactly
/// once - a forged fence marker embedded in one page's own snippet must
/// not add a sixth open/close pair.
#[test]
fn query_hostile_corpus_admits_only_verified_current_non_escaping_hits() {
    let harness = Harness::new();
    support::write_hostile_corpus(&harness.clone_dir(), &harness.outside_dir());
    let response =
        support::write_query_response(&harness.outside_dir(), support::HOSTILE_QUERY_RESPONSE);

    let output = harness.run(
        &["--json", "query", "a hostile corpus stress question"],
        &[("KAIBO_TEST_QMD_QUERY_RESPONSE", response.to_str().unwrap())],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");

    let hits = json["outcome"]["hits"].as_array().expect("hits array");
    let paths: Vec<&str> = hits.iter().map(|h| h["path"].as_str().unwrap()).collect();
    assert_eq!(paths, support::HOSTILE_ADMITTED_PATHS.to_vec());

    for rejected in support::HOSTILE_REJECTED_PATHS {
        assert!(
            !stdout.contains(rejected),
            "rejected path {rejected} leaked into query output"
        );
    }
    assert!(!stdout.contains("/etc/passwd"));
    assert!(!stdout.contains("OUTSIDE CONTENT"));
    assert!(!stdout.contains('\r'));
    assert!(!stdout.contains('\u{7}'));

    let control_chars_hit = hits
        .iter()
        .find(|h| h["path"] == "docs/reference/control-chars.md")
        .expect("control-chars hit present");
    assert_eq!(control_chars_hit["title"], "Hostile Query TitleSecond Line");
    assert_eq!(control_chars_hit["status"], "weirdstatus");

    assert_eq!(stdout.matches("<<<UNTRUSTED CORPUS CONTENT").count(), 5);
    assert_eq!(stdout.matches("<<<END UNTRUSTED CORPUS CONTENT").count(), 5);
}

// --- doctrine -------------------------------------------------------------

#[test]
fn doctrine_explain_runs_nothing_and_exits_success() {
    let harness = Harness::new();

    let output = harness.run(&["--explain", "doctrine", support::DOMAIN], &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(harness.calls().is_empty(), "explain must not run anything");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        7,
        "expected exactly the 7 planned self-heal-pipeline commands, got: {stdout}"
    );
}

#[test]
fn doctrine_reports_success_exit_code_on_a_healthy_corpus() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["doctrine", support::DOMAIN], &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn doctrine_json_output_parses_and_reports_the_exit_code() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["--json", "doctrine", support::DOMAIN], &[]);

    let json = parse_json(&output.stdout);
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["outcome"]["state"], "loaded");
}

/// The same five pages `query` admits, loaded directly off disk this time -
/// `doctrine` walks `docs/reference/` itself rather than trusting a qmd
/// hit, so this exercises the containment check against a real directory
/// listing (including the real escaping symlink) rather than a string in a
/// JSON response.
#[test]
fn doctrine_hostile_corpus_admits_only_verified_current_non_escaping_pages() {
    let harness = Harness::new();
    support::write_hostile_corpus(&harness.clone_dir(), &harness.outside_dir());

    let output = harness.run(&["--json", "doctrine", support::DOMAIN], &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");

    let pages = json["outcome"]["pages"].as_array().expect("pages array");
    let paths: Vec<&str> = pages.iter().map(|p| p["path"].as_str().unwrap()).collect();
    assert_eq!(paths, support::HOSTILE_ADMITTED_PATHS.to_vec());

    for rejected in support::HOSTILE_REJECTED_PATHS {
        assert!(
            !stdout.contains(rejected),
            "rejected path {rejected} leaked into doctrine output"
        );
    }
    assert!(!stdout.contains("OUTSIDE CONTENT"));
    assert!(!stdout.contains('\r'));
    assert!(!stdout.contains('\u{7}'));

    let control_chars_page = pages
        .iter()
        .find(|p| p["path"] == "docs/reference/control-chars.md")
        .expect("control-chars page present");
    assert_eq!(control_chars_page["title"], "WeirdTitle");
    assert_eq!(control_chars_page["status"], "weirdstatus");

    assert_eq!(stdout.matches("<<<UNTRUSTED CORPUS CONTENT").count(), 5);
    assert_eq!(stdout.matches("<<<END UNTRUSTED CORPUS CONTENT").count(), 5);
}

// --- domains --------------------------------------------------------------

#[test]
fn domains_explain_runs_nothing_and_exits_success() {
    let harness = Harness::new();

    let output = harness.run(&["--explain", "domains"], &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(harness.calls().is_empty(), "explain must not run anything");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        7,
        "expected exactly the 7 planned self-heal-pipeline commands, got: {stdout}"
    );
}

#[test]
fn domains_reports_success_exit_code_on_a_healthy_corpus() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["domains"], &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn domains_json_output_parses_and_reports_the_exit_code() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["--json", "domains"], &[]);

    let json = parse_json(&output.stdout);
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["outcome"]["state"], "listed");
}

/// `domains` reads only `_index.md`; it must never be affected by what is
/// (or is not) sitting in `docs/reference/`, hostile or otherwise. Both
/// runs share the exact same `_index.md`; only whether the hostile
/// reference pages exist on disk differs.
#[test]
fn domains_output_is_unaffected_by_hostile_reference_pages() {
    let harness = Harness::new();
    support::write_hostile_moc(&harness.clone_dir());
    let without_pages = harness.run(&["domains"], &[]);

    support::write_hostile_reference_pages(&harness.clone_dir(), &harness.outside_dir());
    let with_pages = harness.run(&["domains"], &[]);

    assert_eq!(without_pages.status.code(), Some(0));
    assert_eq!(without_pages.stdout, with_pages.stdout);
}

// --- lint -----------------------------------------------------------------

/// `lint` shells out to nothing, so its plan is empty and `--explain` has
/// nothing to print. That empty plan is only half the claim: `lint` reads
/// markdown in-process rather than through a stub, so the call log cannot
/// witness a read the way it does for the other verbs. The other half is
/// that the corpus is byte-for-byte and mtime-for-mtime what it was.
#[test]
fn lint_explain_runs_nothing_and_exits_success() {
    let harness = Harness::new();
    let clone = harness.clone_dir();
    support::write_page(
        &clone,
        "docs/reference/page.md",
        support::WELL_FORMED_FRONTMATTER,
        "Body.",
    );
    let page = clone.join("docs/reference/page.md");
    let before = std::fs::read_to_string(&page).unwrap();
    let before_mtime = std::fs::metadata(&page).unwrap().modified().unwrap();

    let output = harness.run(&["--explain", "lint"], &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(harness.calls().is_empty(), "explain must not run anything");
    assert!(
        output.stdout.is_empty(),
        "lint shells out to nothing, so --explain has nothing to print, got: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(before, std::fs::read_to_string(&page).unwrap());
    assert_eq!(
        before_mtime,
        std::fs::metadata(&page).unwrap().modified().unwrap()
    );
}

#[test]
fn lint_reports_success_exit_code_on_a_corpus_with_no_structural_violation() {
    let harness = Harness::new();
    support::write_page(
        &harness.clone_dir(),
        "docs/reference/page.md",
        support::WELL_FORMED_FRONTMATTER,
        "Fine.",
    );

    let output = harness.run(&["lint"], &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A page missing `title`, `tags`, `status` and `updated` trips the
/// structural half of the registry, and a structural violation is bad
/// input: exit 2, the same code a malformed frontmatter block gets.
#[test]
fn lint_reports_usage_exit_code_for_a_structural_violation() {
    let harness = Harness::new();
    support::write_page(
        &harness.clone_dir(),
        "docs/reference/page.md",
        "type: reference",
        "Body.",
    );

    let output = harness.run(&["lint"], &[]);

    assert_eq!(output.status.code(), Some(2));
}

/// No clone on disk at all is unsynced (4), never a gap (3): `lint` has no
/// basis to say whether the corpus it never saw would have had anything to
/// check.
#[test]
fn lint_reports_stale_exit_code_when_the_clone_is_missing() {
    let harness = Harness::new();
    harness.remove_clone();

    let output = harness.run(&["lint"], &[]);

    assert_eq!(output.status.code(), Some(4));
}

/// A `path` argument is corpus-shaped user input, so it is validated for
/// containment before anything is read from it. One walking out of the
/// clone is refused as bad input (2), and the reported outcome is the
/// refusal itself - `invalid_path`, naming the argument. The exit code
/// alone would not distinguish a refusal from having happily linted the
/// file outside the clone and found violations in it, which also exits 2.
#[test]
fn lint_reports_a_path_argument_escaping_the_clone_as_invalid_rather_than_linting_it() {
    let harness = Harness::new();
    support::write_page(
        &harness.clone_dir(),
        "docs/reference/page.md",
        support::WELL_FORMED_FRONTMATTER,
        "Fine.",
    );
    std::fs::write(
        harness.outside_dir().join("secret.md"),
        "OUTSIDE CONTENT - must never be read by kaibo.\n",
    )
    .unwrap();

    let output = harness.run(&["--json", "lint", "../outside/secret.md"], &[]);

    assert_eq!(output.status.code(), Some(2));
    let json = parse_json(&output.stdout);
    assert_eq!(json["outcome"]["state"], "invalid_path");
    assert_eq!(json["outcome"]["path"], "../outside/secret.md");
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("OUTSIDE CONTENT")
    );
}

#[test]
fn lint_json_output_parses_and_reports_the_exit_code() {
    let harness = Harness::new();
    support::write_page(
        &harness.clone_dir(),
        "docs/reference/page.md",
        "type: reference",
        "Body.",
    );

    let output = harness.run(&["--json", "lint"], &[]);

    let json = parse_json(&output.stdout);
    assert_eq!(json["outcome"]["state"], "finished");
    assert_eq!(json["exit_code"], 2);
    assert_eq!(
        output.status.code(),
        Some(2),
        "the reported exit_code and the process exit code must agree"
    );
}

/// The hostile corpus's `forged-tag.md` embeds a carriage return in a
/// frontmatter tag, so that a tool echoing the tag back verbatim prints
/// everything after it as a line of its own - a finding kaibo never made.
/// Stripping the control character defuses that without hiding the tag's
/// own text, which still appears fused onto the real violation's line.
#[test]
fn lint_hostile_corpus_tag_control_character_does_not_forge_an_output_line() {
    let harness = Harness::new();
    support::write_hostile_corpus(&harness.clone_dir(), &harness.outside_dir());

    let output = harness.run(&["lint"], &[]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(!stdout.contains('\r'));
    assert!(
        !stdout
            .lines()
            .any(|line| line.trim() == support::FORGED_TAG_INJECTED_LINE),
        "the forged tag produced a line of its own: {stdout}"
    );
    assert!(
        stdout.contains("forgedinjected: line"),
        "the tag's own text must survive, fused onto the real finding: {stdout}"
    );
}

/// `lint` walks the whole hostile corpus, exactly as `query` and `doctrine`
/// read it. The escaping symlink resolves outside the clone, so it is
/// dropped before it is read rather than reported on - the containment
/// check is the same one, not a copy `lint` grew for itself.
#[test]
fn lint_hostile_corpus_never_reports_a_page_resolving_outside_the_clone() {
    let harness = Harness::new();
    support::write_hostile_corpus(&harness.clone_dir(), &harness.outside_dir());

    let output = harness.run(&["--json", "lint"], &[]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(!stdout.contains("docs/reference/escaping-symlink.md"));
    assert!(!stdout.contains("OUTSIDE CONTENT"));
}

/// The same page carries both a structural violation (its tag is not
/// kebab-case) and a heuristic one (an em dash in its body). The prose
/// finding must stay heuristic regardless of what else is wrong with the
/// file it was found in: severity is the rule's, never the file's.
#[test]
fn lint_hostile_corpus_reports_the_em_dash_as_a_heuristic_finding() {
    let harness = Harness::new();
    support::write_hostile_corpus(&harness.clone_dir(), &harness.outside_dir());

    let output = harness.run(&["--json", "lint"], &[]);

    let json = parse_json(&output.stdout);
    let violations = json["outcome"]["violations"]
        .as_array()
        .expect("violations array");

    let prose_hits: Vec<&serde_json::Value> = violations
        .iter()
        .filter(|v| v["rule_id"] == "prose-style")
        .collect();
    assert!(
        prose_hits
            .iter()
            .any(|v| v["path"] == support::HOSTILE_FORGED_TAG_PATH
                && v["message"].as_str().is_some_and(|m| m.contains("em dash"))),
        "no em-dash prose finding on {}: {violations:#?}",
        support::HOSTILE_FORGED_TAG_PATH
    );
    assert!(prose_hits.iter().all(|v| v["severity"] == "heuristic"));

    assert!(
        violations.iter().any(|v| v["rule_id"] == "tags-kebab-case"
            && v["path"] == support::HOSTILE_FORGED_TAG_PATH
            && v["severity"] == "structural"),
        "the same page's structural finding is what makes the run exit 2"
    );
    assert_eq!(output.status.code(), Some(2));
}

// --- contribute -------------------------------------------------------------

const REPO_ENV: (&str, &str) = ("KAIBO_REPO", "org/knowledge");

#[test]
fn contribute_plan_explain_runs_nothing_and_exits_success() {
    let harness = Harness::new();

    let output = harness.run(
        &["--explain", "contribute", "plan", "how do queries work"],
        &[],
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(harness.calls().is_empty(), "explain must not run anything");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("qmd"),
        "expected the planned qmd query, got: {stdout}"
    );
}

#[test]
fn contribute_apply_explain_runs_nothing_and_exits_success() {
    let harness = Harness::new();

    let output = harness.run(
        &[
            "--explain",
            "contribute",
            "apply",
            "--type",
            "how-to",
            "--domain",
            "docs",
            "--title",
            "A new page",
            "--body",
            "Body text.",
        ],
        &[REPO_ENV],
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(harness.calls().is_empty(), "explain must not run anything");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("git"));
    assert!(stdout.contains("gh"));
}

#[test]
fn contribute_apply_stops_on_a_dirty_clone_without_branching_or_writing() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(
        &[
            "contribute",
            "apply",
            "--type",
            "how-to",
            "--domain",
            "docs",
            "--title",
            "A new page",
            "--body",
            "Body text.",
        ],
        &[
            REPO_ENV,
            (
                "KAIBO_TEST_GIT_STATUS_PORCELAIN",
                " M docs/reference/good.md\n",
            ),
        ],
    );

    assert_eq!(output.status.code(), Some(4));
    assert!(
        !harness
            .clone_dir()
            .join("docs/how-to/a-new-page.md")
            .exists()
    );
    assert!(
        harness.calls().iter().all(|c| !c.contains("checkout -b")),
        "a dirty clone must stop before any branch is created: {:?}",
        harness.calls()
    );
}

#[test]
fn contribute_apply_stops_on_a_structural_lint_failure_and_leaves_the_write_in_place() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(
        &[
            "contribute",
            "apply",
            "--type",
            "how-to",
            "--domain",
            "docs",
            "--title",
            "A new page",
            "--body",
            "Body text.",
            "--tag",
            "NotKebabCase",
        ],
        &[REPO_ENV],
    );

    assert_eq!(output.status.code(), Some(2));
    let written = std::fs::read_to_string(harness.clone_dir().join("docs/how-to/a-new-page.md"))
        .expect("the write is left in place, stop-and-report never discards it");
    assert!(written.contains("NotKebabCase"));
    assert!(
        harness.calls().iter().all(|c| !c.contains("checkout -b")),
        "a lint failure must stop before any branch is created: {:?}",
        harness.calls()
    );
}

#[test]
fn contribute_apply_stops_when_the_fork_parent_does_not_match_the_configured_repo() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(
        &[
            "contribute",
            "apply",
            "--type",
            "how-to",
            "--domain",
            "docs",
            "--title",
            "A new page",
            "--body",
            "Body text.",
            "--tag",
            "good-tag",
        ],
        &[
            REPO_ENV,
            ("KAIBO_TEST_GH_CAN_PUSH", "false"),
            ("KAIBO_TEST_GH_FORK_PARENT", "someone-else/knowledge"),
        ],
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(
        harness.calls().iter().all(|c| !c.contains(" push ")),
        "a mismatched fork parent must never be pushed to: {:?}",
        harness.calls()
    );
    // The clone was still returned to `main`, per the unconditional rule.
    assert!(harness.calls().iter().any(|c| c.contains("checkout main")));
}

#[test]
fn contribute_apply_completes_a_direct_push_contribution_and_opens_a_pr() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(
        &[
            "--json",
            "contribute",
            "apply",
            "--type",
            "how-to",
            "--domain",
            "docs",
            "--title",
            "A new page",
            "--body",
            "Body text.",
            "--tag",
            "good-tag",
        ],
        &[REPO_ENV],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_json(&output.stdout);
    assert_eq!(json["outcome"]["push_route"], "direct");
    assert_eq!(
        json["outcome"]["pr_url"],
        "https://github.com/org/knowledge/pull/1"
    );
    assert!(
        harness
            .clone_dir()
            .join("docs/how-to/a-new-page.md")
            .exists()
    );
    assert!(harness.calls().iter().any(|c| c.contains("checkout main")));
}
