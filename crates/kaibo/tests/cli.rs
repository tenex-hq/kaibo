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
//! that a corpus-reading verb (`query`, `doctrine`) admits only the pages
//! its own trust rules say it should, and that no verb's own output lines
//! or the commands it runs are altered by what the hostile corpus contains.

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
/// appear in `query`'s hits. The four verified, `current`, contained pages
/// must appear, in ranked order, each with its title and status stripped
/// of the control characters it was seeded with, and each fenced exactly
/// once - a forged fence marker embedded in one page's own snippet must
/// not add a fifth open/close pair.
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

    assert_eq!(stdout.matches("<<<UNTRUSTED CORPUS CONTENT").count(), 4);
    assert_eq!(stdout.matches("<<<END UNTRUSTED CORPUS CONTENT").count(), 4);
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

/// The same four pages `query` admits, loaded directly off disk this time -
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

    assert_eq!(stdout.matches("<<<UNTRUSTED CORPUS CONTENT").count(), 4);
    assert_eq!(stdout.matches("<<<END UNTRUSTED CORPUS CONTENT").count(), 4);
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
