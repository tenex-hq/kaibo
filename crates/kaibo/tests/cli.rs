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

/// `--if-stale` is the one flag `sync` takes beyond the global ones, and it
/// has to change what actually runs. The stub `git log` defaults to "just
/// now" when `KAIBO_TEST_GIT_EPOCH` is unset (see `GIT_STUB`), so this
/// harness's corpus reads as fresh without any extra setup - `--if-stale`
/// must skip the pull on exactly that corpus.
#[test]
fn sync_if_stale_flag_skips_the_pull_when_the_corpus_is_already_fresh() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["--json", "sync", "--if-stale"], &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_json(&output.stdout);
    assert_eq!(json["outcome"]["state"], "skipped_fresh");
    assert!(
        harness.calls().iter().all(|c| !c.contains("pull")),
        "a fresh corpus must not be pulled under --if-stale: {:?}",
        harness.calls()
    );
}

/// The other half of the same flag: without it, `sync` pulls regardless of
/// freshness - proving the skip above comes from the flag, not from `sync`
/// never pulling a corpus this fresh in the first place.
#[test]
fn sync_without_if_stale_pulls_even_when_the_corpus_is_already_fresh() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["sync"], &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        harness.calls().iter().any(|c| c.contains("pull")),
        "without --if-stale, sync must always pull: {:?}",
        harness.calls()
    );
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
/// appear in `query`'s hits. The six verified, `current`, contained pages
/// must appear, in ranked order, each with its title and status stripped
/// of the control characters it was seeded with, and each fenced exactly
/// once - a forged fence marker embedded in one page's own snippet must
/// not add a seventh open/close pair.
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

    assert_eq!(stdout.matches("<<<UNTRUSTED CORPUS CONTENT").count(), 6);
    assert_eq!(stdout.matches("<<<END UNTRUSTED CORPUS CONTENT").count(), 6);
}

/// `--include-drafts` is the one flag `query` takes beyond the global ones,
/// and it has to change what actually comes back, not just what the report
/// struct is capable of holding. Deliberately no "draft" anywhere in the
/// fixture's path or title, same reasoning as the unit-test fixture in
/// `kaibo-core`'s `query/tests.rs`: a fixture path containing the word
/// "draft" would let `text.contains("[draft]")` pass even if the flag did
/// nothing at all.
#[test]
fn query_excludes_a_draft_page_by_default_but_include_drafts_surfaces_it_labelled() {
    let harness = Harness::new();
    support::write_page(
        &harness.clone_dir(),
        "docs/reference/onboarding-notes.md",
        "title: Onboarding Notes\nstatus: draft",
        "Body.",
    );
    let response = support::write_query_response(
        &harness.outside_dir(),
        r#"[{"file": "qmd://knowledge/docs/reference/onboarding-notes.md?index=kaibo", "title": "Onboarding Notes", "snippet": "clean snippet", "score": 0.9}]"#,
    );
    let env = [("KAIBO_TEST_QMD_QUERY_RESPONSE", response.to_str().unwrap())];

    let excluded = harness.run(&["query", "a question"], &env);
    assert_eq!(
        excluded.status.code(),
        Some(3),
        "a draft page must be excluded by default (the gap signal), stderr: {}",
        String::from_utf8_lossy(&excluded.stderr)
    );

    let included_json = harness.run(&["--json", "query", "a question", "--include-drafts"], &env);
    assert_eq!(
        included_json.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&included_json.stderr)
    );
    let json = parse_json(&included_json.stdout);
    let hits = json["outcome"]["hits"].as_array().expect("hits array");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["status"], "draft");

    let included_text = harness.run(&["query", "a question", "--include-drafts"], &env);
    let stdout = String::from_utf8(included_text.stdout).unwrap();
    assert!(
        stdout.contains("[draft]"),
        "expected an explicit [draft] label, got: {stdout}"
    );
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

/// The same six pages `query` admits, loaded directly off disk this time -
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

    assert_eq!(stdout.matches("<<<UNTRUSTED CORPUS CONTENT").count(), 6);
    assert_eq!(stdout.matches("<<<END UNTRUSTED CORPUS CONTENT").count(), 6);
}

/// Dedicated coverage for the one hostile page the shared admission test
/// above only counts, not inspects: a body that embeds a `---` line of its
/// own. `frontmatter::parse` stops at the first closing `---`, so this
/// proves the embedded delimiter reaches `doctrine`'s output as ordinary
/// body text on both sides of it, and that fencing it does not add a
/// second open/close marker pair around it.
#[test]
fn doctrine_hostile_corpus_survives_a_body_containing_a_frontmatter_delimiter() {
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
    let page = pages
        .iter()
        .find(|p| p["path"] == "docs/reference/frontmatter-delimiter-in-body.md")
        .expect("frontmatter-delimiter-in-body page admitted");
    assert_eq!(page["title"], "Frontmatter Delimiter Attempt");

    let body = page["body"].as_str().expect("body is a string");
    assert!(
        body.contains("Text before the embedded delimiter."),
        "text before the embedded delimiter must survive, got: {body}"
    );
    assert!(
        body.contains("Text after the embedded delimiter"),
        "text after the embedded delimiter must survive - a naive re-parse \
         truncating at it would drop this, got: {body}"
    );
    assert_eq!(
        body.matches("<<<UNTRUSTED CORPUS CONTENT").count(),
        1,
        "the embedded --- must not add a second fence open marker, got: {body}"
    );
    assert_eq!(
        body.matches("<<<END UNTRUSTED CORPUS CONTENT").count(),
        1,
        "the embedded --- must not add a second fence close marker, got: {body}"
    );
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

/// The hostile page's forged tag is not kebab-case, a structural violation
/// that is what makes the whole run exit 2.
#[test]
fn lint_hostile_corpus_reports_the_forged_tag_as_a_structural_finding() {
    let harness = Harness::new();
    support::write_hostile_corpus(&harness.clone_dir(), &harness.outside_dir());

    let output = harness.run(&["--json", "lint"], &[]);

    let json = parse_json(&output.stdout);
    let violations = json["outcome"]["violations"]
        .as_array()
        .expect("violations array");

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

/// `contribute plan` never appears in the explain-only tests above with a
/// real `gather` run: this exercises the actual candidate-dedup pipeline
/// against the shared hostile corpus. `probe_candidates` never reads a
/// candidate's content (it only maps `file` -> `path` + `score`), so the
/// pages `query`/`doctrine` reject for unreadable or escaping content
/// (`draft-malformed-frontmatter.md`, `escaping-symlink.md`) are legitimate
/// candidates here - the only hits that must never appear are the two whose
/// `file` field never resolves to a repo-relative path at all: a `..`
/// traversal and an absolute path.
#[test]
fn contribute_plan_hostile_corpus_never_surfaces_a_path_traversal_or_absolute_candidate() {
    let harness = Harness::new();
    support::write_hostile_corpus(&harness.clone_dir(), &harness.outside_dir());
    let response =
        support::write_query_response(&harness.outside_dir(), support::HOSTILE_QUERY_RESPONSE);

    let output = harness.run(
        &[
            "--json",
            "contribute",
            "plan",
            "a hostile corpus stress question",
        ],
        &[("KAIBO_TEST_QMD_QUERY_RESPONSE", response.to_str().unwrap())],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = parse_json(&output.stdout);
    assert_eq!(json["outcome"]["state"], "ready");

    let candidates = json["outcome"]["candidates"]
        .as_array()
        .expect("candidates array");
    let paths: Vec<&str> = candidates
        .iter()
        .map(|c| c["path"].as_str().unwrap())
        .collect();

    assert!(
        !paths.iter().any(|p| p.contains("..")),
        "a path-traversal hit leaked into plan candidates: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.starts_with('/')),
        "an absolute-path hit leaked into plan candidates: {paths:?}"
    );
    assert_eq!(
        paths.len(),
        8,
        "expected every hostile hit except the traversal and absolute-path \
         ones, got: {paths:?}"
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

/// `--append` is the flag that changes `contribute apply` from a page
/// creation to an edit: it must write into the named existing page instead
/// of a new file under `<domain>/<type>/<slug>.md`, and the original body
/// must survive alongside the appended text.
#[test]
fn contribute_apply_append_flag_appends_to_the_existing_page_instead_of_creating_one() {
    let harness = Harness::new();
    // Structurally well-formed, since `apply` lint-gates the page it just
    // wrote: `write_minimal_corpus`'s `good.md` is missing `tags`/`updated`
    // and would fail that gate for reasons unrelated to `--append` itself.
    support::write_page(
        &harness.clone_dir(),
        "docs/reference/existing-page.md",
        support::WELL_FORMED_FRONTMATTER,
        "Original body.",
    );

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
            "A page",
            "--body",
            "Appended paragraph.",
            "--append",
            "docs/reference/existing-page.md",
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
    assert_eq!(json["path"], "docs/reference/existing-page.md");
    assert_eq!(json["outcome"]["push_route"], "direct");

    let contents =
        std::fs::read_to_string(harness.clone_dir().join("docs/reference/existing-page.md"))
            .unwrap();
    assert!(
        contents.contains("Original body."),
        "the original body must survive an append, got: {contents}"
    );
    assert!(
        contents.contains("Appended paragraph."),
        "the new body must be appended, got: {contents}"
    );
    assert!(
        !harness.clone_dir().join("docs/how-to/a-page.md").exists(),
        "--append must not also create a new page at the create-mode path"
    );
}

// --- install --------------------------------------------------------------

/// The binary's own version, which `install` stamps into the manifest and
/// `status` compares against. Read from the same place the binary reads it
/// so a release bump cannot leave these tests asserting a stale literal.
const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

fn skill_path(plugin_dir: &std::path::Path, skill: &str) -> std::path::PathBuf {
    plugin_dir.join("skills").join(skill).join("SKILL.md")
}

fn manifest_path(plugin_dir: &std::path::Path) -> std::path::PathBuf {
    plugin_dir.join(".claude-plugin").join("plugin.json")
}

/// The plugin manifest beside `skills/` is what keeps the installed skills
/// namespaced: Claude Code discovers a directory carrying one as a plugin,
/// so its skills stay `kaibo:query` instead of degrading to `query`.
#[test]
fn install_writes_a_plugin_directory_under_the_users_skills_dir() {
    let harness = Harness::new();

    let output = harness.run(&["install"], &[]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plugin_dir = harness.plugin_dir();
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_path(&plugin_dir)).expect("manifest was written"),
    )
    .expect("manifest is valid JSON");
    assert_eq!(manifest["name"], "kaibo");
    assert_eq!(manifest["version"], CLI_VERSION);
    for skill in ["query", "contribute", "sync"] {
        assert!(
            skill_path(&plugin_dir, skill).is_file(),
            "{skill} was not installed"
        );
    }
    assert!(harness.calls().is_empty(), "install shells out to nothing");
}

/// The skills reach disk exactly as the binary carries them, so a reader of
/// the installed file is reading the shipped prose and not a rendering of
/// it.
#[test]
fn every_installed_skill_declares_its_own_name_in_its_frontmatter() {
    let harness = Harness::new();

    harness.run(&["install"], &[]);

    let plugin_dir = harness.plugin_dir();
    for skill in ["query", "contribute", "sync"] {
        let source =
            std::fs::read_to_string(skill_path(&plugin_dir, skill)).expect("skill was installed");
        assert!(
            source.lines().any(|line| line == format!("name: {skill}")),
            "{skill} does not declare its own name: {source}"
        );
    }
}

/// `CLAUDE_CONFIG_DIR` is the documented way to move the install, and the
/// only knob it has. The unit tests can reach it only through a fake
/// environment, so this is the one place the real binary reads the real
/// variable and writes where it points instead of under the home
/// directory.
#[test]
fn claude_config_dir_moves_the_install_off_the_home_directory() {
    let harness = Harness::new();
    let elsewhere = harness.outside_dir().join("claude");

    let output = harness.run(
        &["install"],
        &[("CLAUDE_CONFIG_DIR", elsewhere.to_str().unwrap())],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        manifest_path(&elsewhere.join("skills").join("kaibo")).is_file(),
        "nothing was installed under {}",
        elsewhere.display()
    );
    assert!(
        !harness.plugin_dir().exists(),
        "the home directory was written to as well: {}",
        harness.plugin_dir().display()
    );
}

#[test]
fn installing_twice_changes_nothing_the_second_time() {
    let harness = Harness::new();
    harness.run(&["install"], &[]);
    let before = std::fs::read_to_string(skill_path(&harness.plugin_dir(), "query")).unwrap();

    let output = harness.run(&["--json", "install"], &[]);

    assert_eq!(output.status.code(), Some(0));
    let json = parse_json(&output.stdout);
    for change in json["outcome"]["changes"].as_array().unwrap() {
        assert_eq!(change["action"], "unchanged", "got: {change}");
    }
    assert_eq!(
        before,
        std::fs::read_to_string(skill_path(&harness.plugin_dir(), "query")).unwrap()
    );
}

#[test]
fn install_restores_a_hand_edited_skill_file() {
    let harness = Harness::new();
    harness.run(&["install"], &[]);
    let query = skill_path(&harness.plugin_dir(), "query");
    let shipped = std::fs::read_to_string(&query).unwrap();
    std::fs::write(&query, "hand-edited\n").unwrap();

    harness.run(&["install"], &[]);

    assert_eq!(std::fs::read_to_string(&query).unwrap(), shipped);
}

#[test]
fn install_explain_reports_what_it_would_do_and_writes_nothing() {
    let harness = Harness::new();

    let output = harness.run(&["--explain", "install"], &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(harness.calls().is_empty(), "explain must not run anything");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("result: planned"), "got: {stdout}");
    assert!(
        !harness.plugin_dir().exists(),
        "an explain run must not create {}",
        harness.plugin_dir().display()
    );
}

#[test]
fn uninstall_removes_the_plugin_directory_it_installed() {
    let harness = Harness::new();
    harness.run(&["install"], &[]);

    let output = harness.run(&["install", "--uninstall"], &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        !harness.plugin_dir().exists(),
        "{} survived the uninstall",
        harness.plugin_dir().display()
    );
    assert!(
        harness.home_dir().join(".claude").join("skills").is_dir(),
        "the user's own skills directory is not kaibo's to remove"
    );
}

#[test]
fn uninstalling_twice_is_the_same_as_uninstalling_once() {
    let harness = Harness::new();
    harness.run(&["install"], &[]);
    harness.run(&["install", "--uninstall"], &[]);

    let output = harness.run(&["--json", "install", "--uninstall"], &[]);

    assert_eq!(output.status.code(), Some(0));
    let json = parse_json(&output.stdout);
    for change in json["outcome"]["changes"].as_array().unwrap() {
        assert_eq!(change["action"], "absent", "got: {change}");
    }
}

/// Stop and report, never discard: a directory holding a file kaibo did
/// not install survives, and the report names it.
#[test]
fn uninstall_leaves_behind_a_directory_holding_someone_elses_file() {
    let harness = Harness::new();
    harness.run(&["install"], &[]);
    let stranger = harness
        .plugin_dir()
        .join("skills")
        .join("query")
        .join("NOTES.md");
    std::fs::write(&stranger, "a human put this here\n").unwrap();

    let output = harness.run(&["--json", "install", "--uninstall"], &[]);

    assert_eq!(output.status.code(), Some(0));
    assert!(stranger.is_file(), "the stranger's file was discarded");
    let json = parse_json(&output.stdout);
    assert_eq!(
        json["outcome"]["retained"],
        serde_json::json!([stranger.parent().unwrap().display().to_string()])
    );
}

/// `status` is what keeps the skills and the binary honest with each
/// other: it compares what is installed against what this binary carries.
#[test]
fn status_reports_installed_skills_as_matching_this_binary() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    harness.run(&["install"], &[]);

    let output = harness.run(&["--json", "status"], &[]);

    let json = parse_json(&output.stdout);
    assert_eq!(json["skills"]["installed"], true);
    assert_eq!(json["skills"]["version"], CLI_VERSION);
    assert_eq!(json["skills"]["matches_cli_version"], true);
    assert_eq!(json["skills"]["edited"], serde_json::json!([]));
    assert_eq!(json["skills"]["missing"], serde_json::json!([]));
}

#[test]
fn status_flags_skills_left_behind_by_an_older_binary() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    harness.run(&["install"], &[]);
    std::fs::write(
        manifest_path(&harness.plugin_dir()),
        "{\"name\": \"kaibo\", \"version\": \"0.0.1\"}\n",
    )
    .unwrap();

    let output = harness.run(&["status"], &[]);

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains(&format!(
            "installed skills are version 0.0.1, this binary is {CLI_VERSION}"
        )),
        "got: {stdout}"
    );
    assert!(stdout.contains("next: `kaibo install`"), "got: {stdout}");
}

#[test]
fn status_flags_a_hand_edited_skill_file() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    harness.run(&["install"], &[]);
    std::fs::write(
        skill_path(&harness.plugin_dir(), "contribute"),
        "hand-edited\n",
    )
    .unwrap();

    let output = harness.run(&["--json", "status"], &[]);

    let json = parse_json(&output.stdout);
    assert_eq!(json["skills"]["matches_cli_version"], true);
    assert_eq!(json["skills"]["edited"], serde_json::json!(["contribute"]));
}

#[test]
fn status_on_a_machine_that_never_installed_says_so() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["status"], &[]);

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains(&format!(
            "skills: not installed at {}",
            harness.plugin_dir().display()
        )),
        "got: {stdout}"
    );
    assert!(stdout.contains("next: `kaibo install`"), "got: {stdout}");
}

// --- the scratch home ------------------------------------------------------

fn rust_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).expect("read test source directory") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out
}

/// `install` writes wherever the process's `HOME` points, so a test that
/// spawned the binary itself would install into the developer's real
/// `~/.claude` and overwrite the skills they are actually running.
/// `Harness::run` clears the environment and points `HOME` at a temp dir,
/// and this scans for a second spawn site rather than listing the tests
/// that have to use it, so one added later is covered without being named
/// here.
#[test]
fn the_harness_is_the_only_thing_in_this_suite_that_spawns_the_binary() {
    let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let harness_source = tests_dir.join("support").join("mod.rs");

    // Split so the scan does not find its own needle in this file.
    let marker = concat!("CARGO_BIN", "_EXE");

    let mut violations = Vec::new();
    for path in rust_files(&tests_dir) {
        if path == harness_source {
            continue;
        }
        let contents = std::fs::read_to_string(&path).expect("read test source file");
        if contents.contains(marker) {
            violations.push(
                path.strip_prefix(&tests_dir)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }

    assert!(
        violations.is_empty(),
        "these spawn the kaibo binary outside Harness::run and so inherit this \
         machine's real HOME: {violations:?}"
    );
}

/// The other half of that guardrail: going through the harness has to
/// actually move the install location off this machine. `status` reports
/// where `install` would write, so it is the binary itself saying which
/// home it resolved, and under the harness that is never the real
/// `~/.claude`.
#[test]
fn a_run_through_the_harness_resolves_its_install_location_inside_the_sandbox() {
    let harness = Harness::new();

    let output = harness.run(&["--json", "status"], &[]);

    let json = parse_json(&output.stdout);
    let root = json["skills"]["root"]
        .as_str()
        .expect("status reports the install root")
        .to_string();
    assert_eq!(root, harness.plugin_dir().display().to_string());
    if let Some(real_home) = std::env::var_os("HOME") {
        let real_skills = std::path::Path::new(&real_home).join(".claude");
        assert!(
            !std::path::Path::new(&root).starts_with(&real_skills),
            "a test run resolved {root}, inside the real {}",
            real_skills.display()
        );
    }
}

// --- the paper trail ------------------------------------------------------
//
// The trail is a bystander: it records what the verb did and is never
// allowed to change it. Every test here asserts the verb's own result
// alongside the trail, because a sink that quietly turned a gap into a
// failure would otherwise pass.

#[test]
fn a_query_records_one_event_naming_the_question_and_the_page_it_found() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    let response =
        support::write_query_response(&harness.outside_dir(), support::MINIMAL_QUERY_RESPONSE);

    let output = harness.run(
        &["query", "a question about the fixture"],
        &[("KAIBO_TEST_QMD_QUERY_RESPONSE", response.to_str().unwrap())],
    );

    assert_eq!(output.status.code(), Some(0));
    let events = harness.trail();
    assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
    let attrs = &events[0]["attributes"];
    assert_eq!(events[0]["event_name"], "kaibo.query");
    assert_eq!(attrs["kaibo.subject"], "a question about the fixture");
    assert_eq!(attrs["kaibo.outcome"], "hit");
    assert_eq!(attrs["kaibo.top_hit_path"], "docs/reference/good.md");
    assert_eq!(attrs["process.exit.code"], 0);
    assert_eq!(
        attrs["kaibo.stdout_tty"], false,
        "a piped run is observably not a terminal"
    );
}

#[test]
fn a_query_that_found_nothing_records_a_gap_and_the_verb_still_exits_three() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    // No `KAIBO_TEST_QMD_QUERY_RESPONSE`: the stub answers with `[]`.
    let output = harness.run(&["query", "a question with no answer"], &[]);

    assert_eq!(output.status.code(), Some(3));
    let events = harness.trail();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["attributes"]["kaibo.outcome"], "gap");
    assert_eq!(events[0]["attributes"]["process.exit.code"], 3);
    assert_eq!(events[0]["attributes"]["kaibo.raw_hit_count"], 0);
}

#[test]
fn doctrine_records_the_domain_it_was_asked_to_load() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    harness.run(&["doctrine", support::DOMAIN], &[]);

    let events = harness.trail();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event_name"], "kaibo.doctrine");
    assert_eq!(events[0]["attributes"]["kaibo.domain"], support::DOMAIN);
}

#[test]
fn a_second_invocation_appends_rather_than_replacing_the_first() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    harness.run(&["query", "the first question"], &[]);
    harness.run(&["query", "the second question"], &[]);

    let events = harness.trail();
    assert_eq!(events.len(), 2, "the second run overwrote the first");
    assert_eq!(
        events[0]["attributes"]["kaibo.subject"],
        "the first question"
    );
    assert_eq!(
        events[1]["attributes"]["kaibo.subject"],
        "the second question"
    );
}

#[test]
fn no_log_suppresses_the_trail_without_changing_a_byte_of_output() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    let response =
        support::write_query_response(&harness.outside_dir(), support::MINIMAL_QUERY_RESPONSE);
    let env = [("KAIBO_TEST_QMD_QUERY_RESPONSE", response.to_str().unwrap())];

    let logged = harness.run(&["query", "a question about the fixture"], &env);
    let suppressed = harness.run(&["--no-log", "query", "a question about the fixture"], &env);

    assert_eq!(suppressed.stdout, logged.stdout);
    assert_eq!(suppressed.status.code(), logged.status.code());
    assert_eq!(
        harness.trail().len(),
        1,
        "the `--no-log` run added an event"
    );
}

#[test]
fn the_config_key_suppresses_the_trail_without_anyone_passing_a_flag() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    let output = harness.run(&["query", "a question"], &[("KAIBO_NO_LOG", "1")]);

    assert_eq!(output.status.code(), Some(3));
    assert!(
        !harness.trail_path().exists(),
        "the trail file was created despite KAIBO_NO_LOG"
    );
}

#[test]
fn an_unwritable_trail_leaves_a_successful_query_reporting_success() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    let response =
        support::write_query_response(&harness.outside_dir(), support::MINIMAL_QUERY_RESPONSE);
    let env = [("KAIBO_TEST_QMD_QUERY_RESPONSE", response.to_str().unwrap())];

    let healthy = harness.run(&["--json", "query", "a question about the fixture"], &env);
    harness.block_the_trail();
    let blocked = harness.run(&["--json", "query", "a question about the fixture"], &env);

    assert_eq!(blocked.status.code(), Some(0));
    assert_eq!(
        blocked.stdout, healthy.stdout,
        "a failing trail changed the answer the caller reads"
    );
}

#[test]
fn an_unwritable_trail_neither_manufactures_nor_masks_the_gap_signal() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    harness.block_the_trail();

    let output = harness.run(&["--json", "query", "a question with no answer"], &[]);

    assert_eq!(
        output.status.code(),
        Some(3),
        "exit 3 is the gap signal and belongs to the query, not to the log"
    );
    let json = parse_json(&output.stdout);
    assert_eq!(json["outcome"]["state"], "no_hits");
}

#[test]
fn a_failing_trail_says_so_on_stderr_and_keeps_stdout_machine_readable() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());
    harness.block_the_trail();

    let output = harness.run(&["--json", "query", "a question"], &[]);

    parse_json(&output.stdout);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("trail.jsonl") && stderr.contains("no_log"),
        "a silent failure leaves no way to find out why the trail is empty: {stderr:?}"
    );
}

#[test]
fn explain_writes_no_trail_because_the_verb_never_ran() {
    let harness = Harness::new();
    support::write_minimal_corpus(&harness.clone_dir());

    harness.run(&["--explain", "query", "a question"], &[]);
    harness.run(&["--explain", "doctrine", support::DOMAIN], &[]);

    assert!(
        !harness.trail_path().exists(),
        "`--explain` recorded an invocation that did nothing"
    );
}

// --- OTLP export (feature `otlp`) -----------------------------------------
//
// Compiled only when the feature is on, because the exporter does not exist
// otherwise. `support::FakeCollector` is a loopback listener: nothing here
// leaves the machine, and asserting that nothing was exported needs a real
// listener to be silent, not just a happy exit code.

#[cfg(feature = "otlp")]
mod otlp {
    use super::*;
    use std::time::Duration;
    use support::{FakeCollector, refused_endpoint};

    #[test]
    fn one_event_reaches_the_collector_when_the_feature_and_the_key_are_both_on() {
        let harness = Harness::new();
        support::write_minimal_corpus(&harness.clone_dir());
        let collector = FakeCollector::start();

        let output = harness.run(
            &["query", "a question about the fixture"],
            &[
                ("KAIBO_OTLP_EXPORT", "1"),
                ("OTEL_EXPORTER_OTLP_ENDPOINT", &collector.endpoint()),
            ],
        );

        assert_eq!(output.status.code(), Some(3));
        let requests = collector.requests();
        assert_eq!(requests.len(), 1, "expected exactly one export");
        assert_eq!(requests[0].path, "/v1/logs");
        assert_eq!(requests[0].content_type, "application/x-protobuf");
        assert!(
            String::from_utf8_lossy(&requests[0].body).contains("a question about the fixture"),
            "the exported record does not carry the subject it was built with"
        );
    }

    #[test]
    fn an_endpoint_in_the_environment_exports_nothing_while_the_key_is_off() {
        let harness = Harness::new();
        support::write_minimal_corpus(&harness.clone_dir());
        let collector = FakeCollector::start();

        // `OTEL_EXPORTER_OTLP_ENDPOINT` is commonly exported machine-wide,
        // and `kaibo.subject` is the question someone asked. An ambient
        // variable must not be enough to put it on the wire.
        let output = harness.run(
            &["query", "a question that must stay on this machine"],
            &[("OTEL_EXPORTER_OTLP_ENDPOINT", &collector.endpoint())],
        );

        assert_eq!(output.status.code(), Some(3));
        assert!(
            collector.nothing_arrived_within(Duration::from_millis(300)),
            "an ambient endpoint alone started an export: {:?}",
            collector.requests()
        );
        assert_eq!(
            harness.trail().len(),
            1,
            "the local trail is unconditional and must still have the event"
        );
    }

    #[test]
    fn no_log_suppresses_the_export_as_well_as_the_file() {
        let harness = Harness::new();
        support::write_minimal_corpus(&harness.clone_dir());
        let collector = FakeCollector::start();

        harness.run(
            &["--no-log", "query", "a question"],
            &[
                ("KAIBO_OTLP_EXPORT", "1"),
                ("OTEL_EXPORTER_OTLP_ENDPOINT", &collector.endpoint()),
            ],
        );

        assert!(
            collector.nothing_arrived_within(Duration::from_millis(300)),
            "`--no-log` means this invocation is not recorded, anywhere"
        );
        assert!(!harness.trail_path().exists());
    }

    #[test]
    fn a_collector_refusing_connections_neither_changes_the_exit_code_nor_costs_the_local_trail() {
        let harness = Harness::new();
        support::write_minimal_corpus(&harness.clone_dir());
        let refused = refused_endpoint();

        let output = harness.run(
            &["--json", "query", "a question with no answer"],
            &[
                ("KAIBO_OTLP_EXPORT", "1"),
                ("OTEL_EXPORTER_OTLP_ENDPOINT", &refused),
            ],
        );

        assert_eq!(
            output.status.code(),
            Some(3),
            "exit 3 is the gap signal and belongs to the query, not to the collector"
        );
        assert_eq!(parse_json(&output.stdout)["outcome"]["state"], "no_hits");
        assert_eq!(
            harness.trail().len(),
            1,
            "an unreachable collector must not cost the local line"
        );
    }

    #[test]
    fn a_successful_export_and_a_failed_one_produce_the_same_answer() {
        let harness = Harness::new();
        support::write_minimal_corpus(&harness.clone_dir());
        let response =
            support::write_query_response(&harness.outside_dir(), support::MINIMAL_QUERY_RESPONSE);
        let collector = FakeCollector::start();
        let qmd = ("KAIBO_TEST_QMD_QUERY_RESPONSE", response.to_str().unwrap());

        let exported = harness.run(
            &["--json", "query", "a question about the fixture"],
            &[
                qmd,
                ("KAIBO_OTLP_EXPORT", "1"),
                ("OTEL_EXPORTER_OTLP_ENDPOINT", &collector.endpoint()),
            ],
        );
        let unreachable = harness.run(
            &["--json", "query", "a question about the fixture"],
            &[
                qmd,
                ("KAIBO_OTLP_EXPORT", "1"),
                ("OTEL_EXPORTER_OTLP_ENDPOINT", &refused_endpoint()),
            ],
        );

        assert_eq!(exported.stdout, unreachable.stdout);
        assert_eq!(exported.status.code(), unreachable.status.code());
    }
}

/// The binding flags, at the argument surface. The schema is all or
/// nothing, so each of these is a page `kaibo lint` would have refused,
/// caught before the binary writes, branches or opens a PR.
fn apply_args<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
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
    ];
    args.extend_from_slice(extra);
    args
}

#[test]
fn contribute_apply_refuses_a_binding_page_with_no_severity() {
    let harness = Harness::new();
    let output = harness.run(
        &apply_args(&["--binding", "--action", "file-edit"]),
        &[REPO_ENV],
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(
        harness.calls().is_empty(),
        "nothing may run on a usage error"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--severity"), "stderr was: {stderr}");
}

#[test]
fn contribute_apply_refuses_a_binding_page_with_no_action() {
    let harness = Harness::new();
    let output = harness.run(
        &apply_args(&["--binding", "--severity", "must"]),
        &[REPO_ENV],
    );

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--action"), "stderr was: {stderr}");
}

#[test]
fn contribute_apply_names_the_vocabulary_when_an_action_is_not_one() {
    let harness = Harness::new();
    let output = harness.run(
        &apply_args(&[
            "--binding",
            "--severity",
            "must",
            "--action",
            "pull-request",
        ]),
        &[REPO_ENV],
    );

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("file-edit"), "stderr was: {stderr}");
    assert!(stderr.contains("commit-message"), "stderr was: {stderr}");
}

#[test]
fn contribute_apply_names_the_vocabulary_when_a_severity_is_not_one() {
    let harness = Harness::new();
    let output = harness.run(
        &apply_args(&[
            "--binding",
            "--severity",
            "critical",
            "--action",
            "file-edit",
        ]),
        &[REPO_ENV],
    );

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("must"), "stderr was: {stderr}");
    assert!(stderr.contains("should"), "stderr was: {stderr}");
}

#[test]
fn contribute_apply_refuses_a_severity_that_binds_nothing() {
    // Half a standard is the failure mode the all-or-nothing rule exists
    // for: a page that looks binding and is not.
    let harness = Harness::new();
    let output = harness.run(&apply_args(&["--severity", "must"]), &[REPO_ENV]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--binding"), "stderr was: {stderr}");
}
