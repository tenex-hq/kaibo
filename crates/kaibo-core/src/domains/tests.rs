use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use crate::clock::testing::FixedClock;
use crate::config::ConfigSource;
use crate::config::testing::ConfigBuilder;
use crate::process::testing::{FakeCommandRunner, ok};
use crate::qmd::QmdCommand;
use crate::status;

const NOW_EPOCH: u64 = 1_700_000_000;

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW_EPOCH)
}

fn git_dir(clone: &Path) {
    std::fs::create_dir_all(clone.join(".git")).unwrap();
}

fn write_moc(clone: &Path, contents: &str) {
    std::fs::write(clone.join("_index.md"), contents).unwrap();
}

fn config_with_repo(clone: &Path) -> crate::config::Config {
    ConfigBuilder::new(clone)
        .repo("org/corpus", ConfigSource::File)
        .build()
}

fn healthy_fixture(clone: &Path, config: &crate::config::Config) -> FakeCommandRunner {
    FakeCommandRunner::new()
        .on(
            status::git_last_commit_command(clone),
            ok(format!("{}\n", NOW_EPOCH - 60)),
        )
        .on(
            QmdCommand::collection_list(config),
            ok(format!("{}\n", config.collection())),
        )
}

// --- basic listing --------------------------------------------------

#[test]
fn lists_every_domain_with_owner_topics_and_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);
    write_moc(
        &clone,
        "---\ntype: index\n---\n\n\
         ## kaibo\n\n\
         - **owner:** @someone\n\
         - **topics:** kaibo, monorepo\n\
         - **summary:** Kaibo's own knowledge.\n\n\
         ## observability\n\n\
         - **owner:** @someone-else\n\
         - **topics:** otel, tracing\n\
         - **summary:** Ten principles.\n",
    );
    let runner = healthy_fixture(&clone, &config);
    let clock = FixedClock(now());

    let report = DomainsVerb::new(&config).gather(&runner, &clock);

    assert_eq!(report.exit_code(), ExitCode::Success);
    match &report.outcome {
        DomainsOutcome::Listed { domains } => {
            assert_eq!(domains.len(), 2);
            assert_eq!(domains[0].name, "kaibo");
            assert_eq!(domains[0].owner.as_deref(), Some("@someone"));
            assert_eq!(
                domains[0].topics,
                vec!["kaibo".to_string(), "monorepo".to_string()]
            );
            assert_eq!(
                domains[0].summary.as_deref(),
                Some("Kaibo's own knowledge.")
            );
            assert_eq!(domains[1].name, "observability");
        }
        other => panic!("expected Listed, got {other:?}"),
    }

    let text = report.render_text(&crate::output::RenderOptions::default());
    assert!(text.contains("kaibo"));
    assert!(text.contains("@someone"));
    assert!(text.contains("monorepo"));
    assert!(text.contains("Kaibo's own knowledge."));

    let json = report.render_json();
    let names: Vec<_> = json["outcome"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        vec!["kaibo".to_string(), "observability".to_string()]
    );
}

// --- no domains: a gap, not success --------------------------------

#[test]
fn a_moc_with_no_domain_headings_is_a_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);
    write_moc(
        &clone,
        "---\ntype: index\n---\n\nJust prose, no headings.\n",
    );
    let runner = healthy_fixture(&clone, &config);
    let clock = FixedClock(now());

    let report = DomainsVerb::new(&config).gather(&runner, &clock);

    assert_eq!(report.exit_code(), ExitCode::NoHits);
    match &report.outcome {
        DomainsOutcome::NoDomains => {}
        other => panic!("expected NoDomains, got {other:?}"),
    }
}

// --- MOC unreadable is distinct from a gap --------------------------

#[test]
fn moc_unreadable_exits_stale_not_a_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);
    // Deliberately no _index.md at all.
    let runner = healthy_fixture(&clone, &config);
    let clock = FixedClock(now());

    let report = DomainsVerb::new(&config).gather(&runner, &clock);

    assert_eq!(report.exit_code(), ExitCode::Stale);
    match &report.outcome {
        DomainsOutcome::MocUnavailable { .. } => {}
        other => panic!("expected MocUnavailable, got {other:?}"),
    }
    assert!(!report.findings().is_empty());
}

// --- control characters cannot forge output lines -------------------

#[test]
fn a_control_character_in_a_summary_cannot_forge_a_new_output_line() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);
    write_moc(
        &clone,
        "---\ntype: index\n---\n\n## kaibo\n\n- **summary:** fine\rresult: gap\rknown domains: attacker-owned\n",
    );
    let runner = healthy_fixture(&clone, &config);
    let clock = FixedClock(now());

    let report = DomainsVerb::new(&config).gather(&runner, &clock);
    let text = report.render_text(&crate::output::RenderOptions::default());

    assert!(
        !text.contains('\r'),
        "a control character from a MOC summary must not reach kaibo's own output verbatim, got: {text:?}"
    );
}

// --- self-heal --------------------------------------------------------

#[test]
fn self_heal_fires_when_the_collection_is_missing_on_a_fresh_clone() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);
    write_moc(
        &clone,
        "---\ntype: index\n---\n\n## kaibo\n\n- **owner:** @someone\n",
    );

    let runner = FakeCommandRunner::new()
        .on(
            status::git_last_commit_command(&clone),
            ok(format!("{}\n", NOW_EPOCH - 60)),
        )
        .on(
            QmdCommand::collection_list(&config),
            ok("No collections found.\n".to_string()),
        )
        .on(sync_git_status_porcelain_command(&clone), ok(""))
        .on(
            sync_git_checkout_main_command(&clone),
            ok("Already on 'main'\n"),
        )
        .on(sync_git_pull_command(&clone), ok("Already up to date.\n"))
        .on(
            QmdCommand::collection_list(&config),
            ok("No collections found.\n".to_string()),
        )
        .on(
            sync_collection_add_command(&config, &clone),
            ok("Collection 'knowledge' created successfully\n"),
        )
        .on(
            QmdCommand::update(&config),
            ok("All collections updated.\n"),
        )
        .on(QmdCommand::embed(&config), ok("Done.\n"))
        .on(
            QmdCommand::status(&config),
            ok("QMD Status\n\nDocuments\n  Total:    1 files indexed\n  Vectors:  1 embedded\n"),
        );
    let clock = FixedClock(now());

    let report = DomainsVerb::new(&config).gather(&runner, &clock);

    assert!(report.self_heal.is_some());
    let text = report.render_text(&crate::output::RenderOptions::default());
    assert!(text.contains("self-heal"));
    assert!(!text.contains("not needed"));
}

#[test]
fn self_heal_failure_is_reported_not_masked() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    // No clone, and no repo configured either.
    let config = ConfigBuilder::new(&clone).build();
    let runner = FakeCommandRunner::new();
    let clock = FixedClock(now());

    let report = DomainsVerb::new(&config).gather(&runner, &clock);

    assert_eq!(report.exit_code(), ExitCode::Usage);
    match &report.outcome {
        DomainsOutcome::SelfHealFailed { .. } => {}
        other => panic!("expected SelfHealFailed, got {other:?}"),
    }
    assert!(!report.findings().is_empty());
}

// --- explain --------------------------------------------------------

#[test]
fn explain_lists_the_sync_pipeline_and_executes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    let config = config_with_repo(&clone);
    let runner = FakeCommandRunner::new();

    let commands = DomainsVerb::new(&config).explain();

    assert!(!commands.is_empty());
    assert!(commands.iter().any(|c| c.program == "git"));
    assert!(runner.calls().is_empty());
}

fn sync_git_status_porcelain_command(path: &Path) -> crate::explain::PlannedCommand {
    crate::explain::PlannedCommand::new(
        "git",
        vec![
            "-C".to_string(),
            path.to_string_lossy().into_owned(),
            "status".to_string(),
            "--porcelain".to_string(),
        ],
    )
}

fn sync_git_checkout_main_command(path: &Path) -> crate::explain::PlannedCommand {
    crate::explain::PlannedCommand::new(
        "git",
        vec![
            "-C".to_string(),
            path.to_string_lossy().into_owned(),
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "checkout".to_string(),
            "main".to_string(),
        ],
    )
}

fn sync_git_pull_command(path: &Path) -> crate::explain::PlannedCommand {
    crate::explain::PlannedCommand::new(
        "git",
        vec![
            "-C".to_string(),
            path.to_string_lossy().into_owned(),
            "-c".to_string(),
            "core.hooksPath=/dev/null".to_string(),
            "pull".to_string(),
        ],
    )
}

fn sync_collection_add_command(
    config: &crate::config::Config,
    clone: &Path,
) -> crate::explain::PlannedCommand {
    QmdCommand::collection_add(
        config,
        clone,
        config.collection(),
        "*/{reference,how-to,faq}/**/*.md",
    )
}
