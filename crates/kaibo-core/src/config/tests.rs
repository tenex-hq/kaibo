use super::*;
use std::collections::HashMap as StdHashMap;

/// Test-only [`Environment`]: explicit values in, no ambient state read.
/// This is the injection the hermetic-test doctrine asks for, so
/// env-reading tests can run concurrently without racing on the real
/// process environment.
struct FakeEnvironment {
    vars: StdHashMap<String, String>,
    home: Option<PathBuf>,
}

impl FakeEnvironment {
    fn new(home: Option<PathBuf>) -> Self {
        Self {
            vars: StdHashMap::new(),
            home,
        }
    }

    fn with_var(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_string(), value.to_string());
        self
    }
}

impl Environment for FakeEnvironment {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }
}

fn write_config_file(home: &Path, contents: &str) {
    let dir = home.join(".kaibo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), contents).unwrap();
}

#[test]
fn defaults_when_nothing_set() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()));
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.repo(), None);
    assert_eq!(
        config.clone_path(),
        tmp.path().join(".kaibo").join("knowledge")
    );
    assert_eq!(config.index(), DEFAULT_INDEX);
    assert_eq!(config.collection(), DEFAULT_COLLECTION);
    assert_eq!(config.api_url(), None);

    for key in [
        ConfigKey::Repo,
        ConfigKey::Clone,
        ConfigKey::Index,
        ConfigKey::Collection,
        ConfigKey::ApiUrl,
    ] {
        assert_eq!(config.source(key), ConfigSource::Default);
    }
}

#[test]
fn file_overrides_default_per_key() {
    let tmp = tempfile::tempdir().unwrap();
    write_config_file(
        tmp.path(),
        "repo = \"org/corpus\"\nindex = \"custom-index\"\n",
    );
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()));
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.repo(), Some("org/corpus"));
    assert_eq!(config.source(ConfigKey::Repo), ConfigSource::File);
    assert_eq!(config.index(), "custom-index");
    assert_eq!(config.source(ConfigKey::Index), ConfigSource::File);

    // Keys the file did not set still fall back to compiled defaults.
    assert_eq!(config.collection(), DEFAULT_COLLECTION);
    assert_eq!(config.source(ConfigKey::Collection), ConfigSource::Default);
}

#[test]
fn env_overrides_file_per_key() {
    let tmp = tempfile::tempdir().unwrap();
    write_config_file(tmp.path(), "repo = \"org/corpus\"\n");
    let env =
        FakeEnvironment::new(Some(tmp.path().to_path_buf())).with_var(ENV_REPO, "org/from-env");
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.repo(), Some("org/from-env"));
    assert_eq!(config.source(ConfigKey::Repo), ConfigSource::Env);
}

#[test]
fn env_overrides_default_per_key() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
        .with_var(ENV_COLLECTION, "from-env-collection");
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.collection(), "from-env-collection");
    assert_eq!(config.source(ConfigKey::Collection), ConfigSource::Env);
}

/// An exported-but-blank variable must fall through, not shadow the
/// layer below it with a value no verb could use.
#[test]
fn empty_env_value_falls_through_to_file_then_default() {
    let tmp = tempfile::tempdir().unwrap();
    write_config_file(tmp.path(), "collection = \"from-file-collection\"\n");
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
        .with_var(ENV_COLLECTION, "")
        .with_var(ENV_INDEX, "");
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.collection(), "from-file-collection");
    assert_eq!(config.source(ConfigKey::Collection), ConfigSource::File);
    assert_eq!(config.index(), DEFAULT_INDEX);
    assert_eq!(config.source(ConfigKey::Index), ConfigSource::Default);
}

/// `repo` has no default, so an empty value must leave it `None` rather
/// than `Some("")` - otherwise the "unset repo" check every verb makes
/// would have to test for two different empty states.
#[test]
fn empty_repo_resolves_to_none_not_empty_string() {
    let tmp = tempfile::tempdir().unwrap();
    write_config_file(tmp.path(), "repo = \"\"\n");
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf())).with_var(ENV_REPO, "");
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.repo(), None);
    assert_eq!(config.source(ConfigKey::Repo), ConfigSource::Default);
}

#[test]
fn config_source_displays_as_a_lowercase_label() {
    assert_eq!(ConfigSource::Env.to_string(), "env");
    assert_eq!(ConfigSource::File.to_string(), "file");
    assert_eq!(ConfigSource::Default.to_string(), "default");
}

#[test]
fn missing_home_dir_without_explicit_clone_is_an_error() {
    let env = FakeEnvironment::new(None);
    let err = Config::resolve_with(&env).unwrap_err();
    assert!(matches!(err, ConfigError::NoHomeDir));
}

/// `install` writes where Claude Code reads, and Claude Code reads
/// `CLAUDE_CONFIG_DIR` when it is set - so kaibo has to read it too, or a
/// user who has moved that directory gets a plugin installed somewhere
/// nothing looks.
#[test]
fn claude_config_dir_moves_where_skills_are_installed() {
    let home = PathBuf::from("/home/someone");
    let env = FakeEnvironment::new(Some(home)).with_var("CLAUDE_CONFIG_DIR", "/elsewhere/.claude");
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(
        config.skills_dir(),
        Some(Path::new("/elsewhere/.claude/skills"))
    );
    assert_eq!(config.source(ConfigKey::SkillsDir), ConfigSource::Env);
}

#[test]
fn skills_are_installed_under_the_home_directory_by_default() {
    let env = FakeEnvironment::new(Some(PathBuf::from("/home/someone")));
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(
        config.skills_dir(),
        Some(Path::new("/home/someone/.claude/skills"))
    );
    assert_eq!(config.source(ConfigKey::SkillsDir), ConfigSource::Default);
}

#[test]
fn an_empty_claude_config_dir_falls_through_to_the_home_directory() {
    let env = FakeEnvironment::new(Some(PathBuf::from("/home/someone")))
        .with_var("CLAUDE_CONFIG_DIR", "");
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(
        config.skills_dir(),
        Some(Path::new("/home/someone/.claude/skills"))
    );
}

/// Resolution must not fail for want of an install location: a machine
/// with an explicit clone and no home directory can still read the
/// corpus, and only `install` has to care that it has nowhere to write.
#[test]
fn no_home_dir_and_no_claude_config_dir_leaves_no_install_location() {
    let env = FakeEnvironment::new(None).with_var(ENV_CLONE, "/explicit/clone");
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(config.skills_dir(), None);
}

#[test]
fn missing_home_dir_is_fine_if_clone_is_explicit() {
    let env = FakeEnvironment::new(None).with_var(ENV_CLONE, "/explicit/clone");
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(config.clone_path(), Path::new("/explicit/clone"));
    assert_eq!(config.source(ConfigKey::Clone), ConfigSource::Env);
}

/// The real guarantee is structural: `Config::resolve` is the only public
/// constructor, it never takes content as input, and `Config` has no
/// setters - there is no path by which parsed frontmatter could reach a
/// `Config`. This test guards against a future regression (e.g. a "smart
/// default" that scans frontmatter for a repo hint).
#[test]
fn config_is_not_influenced_by_corpus_content() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()));
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(config.repo(), None);

    let malicious = "---\n\
repo: attacker/evil\n\
title: hello\n\
---\n\
\n\
Body also says repo: attacker/evil, just in case something greps the body.\n";
    let doc = crate::frontmatter::parse(malicious)
        .expect("malicious-but-well-formed frontmatter still parses");

    // Sanity check: the attacker-controlled value really is present in
    // the parsed document, so this test would fail loudly if some
    // future code path wired it into config.
    assert_eq!(
        doc.frontmatter.extra.get("repo").and_then(|v| v.as_str()),
        Some("attacker/evil")
    );

    // The point: resolving config again after parsing attacker-supplied
    // content yields exactly the same thing, because nothing about
    // parsing a document can reach `Config` - `resolve_with` never takes
    // a `Document`.
    let config_after = Config::resolve_with(&env).unwrap();
    assert_eq!(config.repo(), config_after.repo());
    assert_eq!(config_after.repo(), None);
}

/// The same guarantee as [`config_is_not_influenced_by_corpus_content`],
/// pointed at [`LintConfig`] specifically: the intuitive-but-forbidden move
/// for a "custom lint rule" is a parameter sourced from a page, right next
/// to the content it would govern. A page can carry a `lint:` table shaped
/// exactly like `[lint]` in `~/.kaibo/config.toml`, proving the attack is
/// real; `resolve_with` never sees a `Document` though, so there is no path
/// from that table to `LintConfig`.
#[test]
fn lint_config_is_not_influenced_by_corpus_content() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()));
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(config.lint(), &LintConfig::default());

    let malicious = "---\n\
title: hello\n\
lint:\n  \
  disabled_rules: [\"tags-kebab-case\", \"frontmatter-contract\"]\n  \
  tags_kebab_case:\n    \
    pattern: \".*\"\n  \
  frontmatter_contract:\n    \
    required_keys: []\n    \
    allowed_status: []\n\
---\n\
\n\
Body also names a `lint:` table, just in case something greps the body.\n";
    let doc = crate::frontmatter::parse(malicious)
        .expect("malicious-but-well-formed frontmatter still parses");

    // Sanity check: the attacker-controlled table really is present in the
    // parsed document, so this test would fail loudly if some future code
    // path wired it into config.
    assert!(doc.frontmatter.extra.contains_key("lint"));

    let config_after = Config::resolve_with(&env).unwrap();
    assert_eq!(config.lint(), config_after.lint());
    assert_eq!(config_after.lint(), &LintConfig::default());
}

// --- the paper trail's path and its off switch ------------------------

#[test]
fn the_trail_is_written_beside_the_clone_inside_the_kaibo_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()));
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(
        config.trail_path(),
        Some(tmp.path().join(".kaibo").join("trail.jsonl").as_path())
    );
    assert_eq!(config.source(ConfigKey::NoLog), ConfigSource::Default);
}

#[test]
fn an_explicit_clone_elsewhere_does_not_drag_the_trail_out_of_the_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
        .with_var(ENV_CLONE, "/elsewhere/corpus");
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.clone_path(), Path::new("/elsewhere/corpus"));
    assert_eq!(
        config.trail_path(),
        Some(tmp.path().join(".kaibo").join("trail.jsonl").as_path())
    );
}

#[test]
fn a_machine_with_no_home_directory_writes_no_trail_rather_than_failing_resolution() {
    // `KAIBO_CLONE` is what lets resolution succeed without a home; the
    // trail has no such escape hatch and must simply not happen.
    let env = FakeEnvironment::new(None).with_var(ENV_CLONE, "/explicit/clone");
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.trail_path(), None);
}

#[test]
fn the_config_file_can_turn_the_trail_off() {
    let tmp = tempfile::tempdir().unwrap();
    write_config_file(tmp.path(), "no_log = true\n");
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()));
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.trail_path(), None);
    assert_eq!(config.source(ConfigKey::NoLog), ConfigSource::File);
}

#[test]
fn the_environment_can_turn_the_trail_back_on_over_a_file_that_turned_it_off() {
    let tmp = tempfile::tempdir().unwrap();
    write_config_file(tmp.path(), "no_log = true\n");
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf())).with_var(ENV_NO_LOG, "0");
    let config = Config::resolve_with(&env).unwrap();

    assert!(config.trail_path().is_some());
    assert_eq!(config.source(ConfigKey::NoLog), ConfigSource::Env);
}

#[test]
fn a_word_that_is_not_a_boolean_falls_through_instead_of_silently_disabling_the_trail() {
    // `KAIBO_NO_LOG=maybe` is a typo, not a decision. Reading any non-empty
    // value as "on" would let one silently stop the trail for good.
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf())).with_var(ENV_NO_LOG, "maybe");
    let config = Config::resolve_with(&env).unwrap();

    assert!(config.trail_path().is_some());
    assert_eq!(config.source(ConfigKey::NoLog), ConfigSource::Default);
}

#[test]
fn every_spelling_of_yes_turns_the_trail_off_and_every_spelling_of_no_leaves_it_on() {
    let tmp = tempfile::tempdir().unwrap();

    for on in ["1", "true", "TRUE", "yes", "on"] {
        let env = FakeEnvironment::new(Some(tmp.path().to_path_buf())).with_var(ENV_NO_LOG, on);
        assert_eq!(
            Config::resolve_with(&env).unwrap().trail_path(),
            None,
            "`KAIBO_NO_LOG={on}` should have disabled the trail"
        );
    }

    for off in ["0", "false", "FALSE", "no", "off"] {
        let env = FakeEnvironment::new(Some(tmp.path().to_path_buf())).with_var(ENV_NO_LOG, off);
        assert!(
            Config::resolve_with(&env).unwrap().trail_path().is_some(),
            "`KAIBO_NO_LOG={off}` should have left the trail on"
        );
    }
}

// --- the OTLP gate ----------------------------------------------------

#[test]
fn nothing_is_exported_until_kaibo_own_key_names_a_collector() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()));
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(config.otlp(), None);
    assert_eq!(config.otlp_endpoint(), None);
    assert_eq!(
        config.source(ConfigKey::OtlpEndpoint),
        ConfigSource::Default
    );
}

#[test]
fn an_endpoint_in_the_ambient_environment_does_not_by_itself_start_exporting() {
    // `OTEL_EXPORTER_OTLP_ENDPOINT` is commonly exported machine-wide, and
    // the event carries the question someone asked. Honouring it as a
    // trigger would send that off the box because a shell profile said so.
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
        .with_var(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "http://collector.internal:4318",
        )
        .with_var(
            "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
            "http://collector.internal:4318/v1/logs",
        )
        .with_var("OTEL_SDK_DISABLED", "false");

    assert_eq!(Config::resolve_with(&env).unwrap().otlp(), None);
}

#[test]
fn naming_a_collector_in_the_environment_exports_to_its_logs_path() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
        .with_var("KAIBO_OTLP_ENDPOINT", "http://localhost:4320");
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(
        config.otlp(),
        Some(OtlpTarget {
            logs_endpoint: "http://localhost:4320/v1/logs".to_string(),
            timeout: Duration::from_millis(2_000),
        })
    );
    assert_eq!(config.otlp_endpoint(), Some("http://localhost:4320"));
    assert_eq!(config.source(ConfigKey::OtlpEndpoint), ConfigSource::Env);
}

#[test]
fn the_config_file_can_name_the_collector_without_an_environment_variable() {
    let tmp = tempfile::tempdir().unwrap();
    write_config_file(tmp.path(), "otlp_endpoint = \"http://localhost:4320\"\n");
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()));
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(
        config.otlp().map(|target| target.logs_endpoint),
        Some("http://localhost:4320/v1/logs".to_string())
    );
    assert_eq!(config.source(ConfigKey::OtlpEndpoint), ConfigSource::File);
}

#[test]
fn the_environment_names_a_different_collector_over_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    write_config_file(tmp.path(), "otlp_endpoint = \"http://localhost:4320\"\n");
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
        .with_var("KAIBO_OTLP_ENDPOINT", "http://otel.example:4318");
    let config = Config::resolve_with(&env).unwrap();

    assert_eq!(
        config.otlp().map(|target| target.logs_endpoint),
        Some("http://otel.example:4318/v1/logs".to_string())
    );
    assert_eq!(config.source(ConfigKey::OtlpEndpoint), ConfigSource::Env);
}

#[test]
fn kaibo_own_key_picks_the_collector_even_when_the_ambient_one_points_elsewhere() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
        .with_var(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "http://collector.internal:4318",
        )
        .with_var("KAIBO_OTLP_ENDPOINT", "http://localhost:4320");

    assert_eq!(
        Config::resolve_with(&env)
            .unwrap()
            .otlp()
            .map(|target| target.logs_endpoint),
        Some("http://localhost:4320/v1/logs".to_string())
    );
}

#[test]
fn the_logs_path_is_appended_to_the_base_the_way_the_otlp_specification_appends_it() {
    // A trailing slash must not double up, and a base with a path of its own
    // (a collector behind a reverse proxy) keeps that path.
    let tmp = tempfile::tempdir().unwrap();
    for (base, expected) in [
        ("http://localhost:4320/", "http://localhost:4320/v1/logs"),
        (
            "http://otel.example/ingest",
            "http://otel.example/ingest/v1/logs",
        ),
        (
            "http://otel.example/ingest/",
            "http://otel.example/ingest/v1/logs",
        ),
    ] {
        let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
            .with_var("KAIBO_OTLP_ENDPOINT", base);
        assert_eq!(
            Config::resolve_with(&env)
                .unwrap()
                .otlp()
                .map(|target| target.logs_endpoint),
            Some(expected.to_string()),
            "base `{base}`"
        );
    }
}

#[test]
fn an_empty_endpoint_counts_as_unset_at_every_layer() {
    let tmp = tempfile::tempdir().unwrap();
    write_config_file(tmp.path(), "otlp_endpoint = \"http://localhost:4320\"\n");
    let env =
        FakeEnvironment::new(Some(tmp.path().to_path_buf())).with_var("KAIBO_OTLP_ENDPOINT", "");
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(config.otlp_endpoint(), Some("http://localhost:4320"));
    assert_eq!(config.source(ConfigKey::OtlpEndpoint), ConfigSource::File);

    write_config_file(tmp.path(), "otlp_endpoint = \"\"\n");
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(config.otlp(), None);
    assert_eq!(
        config.source(ConfigKey::OtlpEndpoint),
        ConfigSource::Default
    );
}

#[test]
fn a_config_file_still_carrying_the_retired_switch_is_refused_with_the_replacement() {
    // `otlp_export = true` once meant "send". Ignoring it now would leave
    // someone believing they export while nothing leaves the machine, so the
    // key is refused, and the refusal says what to write instead.
    let tmp = tempfile::tempdir().unwrap();
    for value in ["true", "false"] {
        write_config_file(tmp.path(), &format!("otlp_export = {value}\n"));
        let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()));

        let err = Config::resolve_with(&env).unwrap_err();

        assert_eq!(err.exit_code(), ExitCode::Usage);
        let message = err.to_string();
        assert!(
            message.contains("`otlp_export`")
                && message.contains("otlp_endpoint = \"http://localhost:4318\"")
                && message.contains("delete the line"),
            "`otlp_export = {value}` must be refused with its replacement: {message}"
        );
        assert!(
            message.contains(
                &tmp.path()
                    .join(".kaibo")
                    .join("config.toml")
                    .display()
                    .to_string()
            ),
            "the refusal has to say which file: {message}"
        );
    }
}

#[test]
fn the_retired_environment_switch_is_refused_with_the_replacement() {
    let tmp = tempfile::tempdir().unwrap();
    let env =
        FakeEnvironment::new(Some(tmp.path().to_path_buf())).with_var("KAIBO_OTLP_EXPORT", "1");

    let err = Config::resolve_with(&env).unwrap_err();

    assert_eq!(err.exit_code(), ExitCode::Usage);
    assert_eq!(
        err.to_string(),
        "KAIBO_OTLP_EXPORT is no longer read: export is switched on by naming the \
         collector. Use `export KAIBO_OTLP_ENDPOINT=http://localhost:4318` (your \
         collector's OTLP/HTTP base URL), or `unset KAIBO_OTLP_EXPORT` to keep export off"
    );
}

#[test]
fn an_empty_retired_environment_switch_is_unset_rather_than_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let env =
        FakeEnvironment::new(Some(tmp.path().to_path_buf())).with_var("KAIBO_OTLP_EXPORT", "");

    assert_eq!(Config::resolve_with(&env).unwrap().otlp(), None);
}

#[test]
fn export_waits_two_seconds_by_default_rather_than_the_specification_ten() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
        .with_var("KAIBO_OTLP_ENDPOINT", "http://localhost:4320");

    assert_eq!(
        Config::resolve_with(&env).unwrap().otlp().unwrap().timeout,
        Duration::from_millis(2_000),
        "an agent waits on this command; ten seconds for a collector that \
         has gone away is not a CLI default"
    );
}

#[test]
fn an_explicit_otel_timeout_still_wins_because_the_variable_is_the_standard_one() {
    let tmp = tempfile::tempdir().unwrap();
    let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
        .with_var("KAIBO_OTLP_ENDPOINT", "http://localhost:4320")
        .with_var("OTEL_EXPORTER_OTLP_TIMEOUT", "350");

    assert_eq!(
        Config::resolve_with(&env).unwrap().otlp().unwrap().timeout,
        Duration::from_millis(350)
    );
}

#[test]
fn a_timeout_that_is_not_a_positive_number_falls_back_instead_of_blocking_forever() {
    let tmp = tempfile::tempdir().unwrap();
    for nonsense in ["0", "-1", "soon", ""] {
        let env = FakeEnvironment::new(Some(tmp.path().to_path_buf()))
            .with_var("KAIBO_OTLP_ENDPOINT", "http://localhost:4320")
            .with_var("OTEL_EXPORTER_OTLP_TIMEOUT", nonsense);
        assert_eq!(
            Config::resolve_with(&env).unwrap().otlp().unwrap().timeout,
            Duration::from_millis(2_000),
            "`OTEL_EXPORTER_OTLP_TIMEOUT={nonsense}` should not have been honoured"
        );
    }
}
