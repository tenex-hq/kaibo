// Excerpt from crates/kaibo-core/src/domains/tests.rs, copied verbatim.
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
const KAIBO_SUMMARY: &str = "Kaibo's own knowledge.";

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
    let last_commit = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 60;
    FakeCommandRunner::new()
        .on(
            status::git_last_commit_command(clone),
            ok(format!("{last_commit}\n")),
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
        &format!(
            "---\ntype: index\n---\n\n\
             ## kaibo\n\n\
             - **owner:** @someone\n\
             - **topics:** kaibo, monorepo\n\
             - **summary:** {KAIBO_SUMMARY}\n\n\
             ## observability\n\n\
             - **owner:** @someone-else\n\
             - **topics:** otel, tracing\n\
             - **summary:** Ten principles.\n"
        ),
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
            assert_eq!(domains[0].summary.as_deref(), Some(KAIBO_SUMMARY));
            assert_eq!(domains[1].name, "observability");
        }
        other => panic!("expected Listed, got {other:?}"),
    }

    let text = report.render_text(&crate::output::RenderOptions::default());
    assert!(text.contains("kaibo"));
    assert!(text.contains("@someone"));
    assert!(text.contains("monorepo"));
    assert!(text.contains(KAIBO_SUMMARY));

    let json = report.render_json();
    let names: Vec<_> = json["outcome"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap().to_string())
        .collect();
    let rendered: Vec<String> = match &report.outcome {
        DomainsOutcome::Listed { domains } => domains.iter().map(|d| d.name.clone()).collect(),
        other => panic!("expected Listed, got {other:?}"),
    };
    assert_eq!(names, rendered);
}

// --- no domains: a gap, not success --------------------------------

#[test]
fn a_moc_with_no_domain_headings_is_a_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("clone");
    git_dir(&clone);
    let config = config_with_repo(&clone);
    write_moc(&clone, "Just prose, no headings.\n");
    let runner = healthy_fixture(&clone, &config);
    let clock = FixedClock(now());

    let report = DomainsVerb::new(&config).gather(&runner, &clock);

    assert_eq!(report.exit_code(), ExitCode::NoHits);
    match &report.outcome {
        DomainsOutcome::NoDomains => {}
        other => panic!("expected NoDomains, got {other:?}"),
    }
}
