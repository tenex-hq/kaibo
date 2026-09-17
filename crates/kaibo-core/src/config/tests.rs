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

#[test]
fn missing_home_dir_is_fine_if_clone_is_explicit() {
    let env = FakeEnvironment::new(None).with_var(ENV_CLONE, "/explicit/clone");
    let config = Config::resolve_with(&env).unwrap();
    assert_eq!(config.clone_path(), Path::new("/explicit/clone"));
    assert_eq!(config.source(ConfigKey::Clone), ConfigSource::Env);
}

/// Mandatory test: config cannot be influenced by content.
///
/// The real guarantee is structural, not this test: `Config::resolve` is
/// the only public constructor, it never takes a document or content of
/// any kind as input, and `Config` has no setters - so there is no path
/// by which parsed frontmatter could reach a `Config` at all. This test
/// guards against a future regression (e.g. someone later wiring a
/// "smart default" that scans frontmatter for a repo hint), it does not
/// discover a design flaw.
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
