//! Config resolution.
//!
//! Precedence, highest first: `KAIBO_*` environment variables, then
//! `~/.kaibo/config.toml`, then compiled defaults.
//!
//! **Invariant: config comes from configuration, never from content.** A
//! repo name appearing inside a retrieved page cannot reach the resolved
//! config, because by the time any corpus content exists in the process,
//! config is already resolved and frozen. Concretely:
//!
//! - [`Config::resolve`] is the *only* public constructor. It is the only
//!   place in this module that calls `std::env::var` or reads
//!   `~/.kaibo/config.toml`.
//! - `Config` has no public mutable fields and no setters. Read it through
//!   getters; pass it around as `&Config`.
//! - Every other way to build a `Config` (used only by this module's tests)
//!   takes explicit values and cannot read ambient state.
//!
//! Callers must resolve config exactly once, in `main`, before opening any
//! corpus file, and treat the result as immutable for the life of the
//! process.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{ExitCode, ExitCoded};

const ENV_REPO: &str = "KAIBO_REPO";
const ENV_CLONE: &str = "KAIBO_CLONE";
const ENV_INDEX: &str = "KAIBO_INDEX";
const ENV_COLLECTION: &str = "KAIBO_COLLECTION";
const ENV_API_URL: &str = "KAIBO_API_URL";

const DEFAULT_INDEX: &str = "kaibo";
const DEFAULT_COLLECTION: &str = "knowledge";

/// Which of the three layers a resolved value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Env,
    File,
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ConfigSource::Env => "env",
            ConfigSource::File => "file",
            ConfigSource::Default => "default",
        })
    }
}

/// A resolvable config key, for reporting where each value came from via
/// [`Config::source`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigKey {
    Repo,
    Clone,
    Index,
    Collection,
    ApiUrl,
}

/// Failure while resolving config. Resolution runs before anything else, so
/// these are the only errors that can happen this early.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    // Errors are instructions: this names the exact next step, because there
    // is no further fallback to try once the home directory is unknown.
    #[error(
        "could not determine a home directory to resolve `~/.kaibo`; \
         set KAIBO_CLONE explicitly, e.g. `export KAIBO_CLONE=/path/to/clone`"
    )]
    NoHomeDir,

    #[error("failed to read config file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse config file {path} as TOML: {source}")]
    ParseFile {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
}

impl ExitCoded for ConfigError {
    fn exit_code(&self) -> ExitCode {
        // A missing home dir or a broken config file is the user's
        // environment needing a fix, not a bug in kaibo: usage error.
        ExitCode::Usage
    }
}

/// Raw values read from `~/.kaibo/config.toml`. Every field is optional: a
/// config file may set only the keys it cares about.
#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    repo: Option<String>,
    clone: Option<PathBuf>,
    index: Option<String>,
    collection: Option<String>,
    api_url: Option<String>,
}

/// Where [`Config::resolve`] reads ambient state from.
///
/// [`ProcessEnvironment`] is the only implementation used outside tests, and
/// it is the only place in the crate that calls `std::env::var` or looks at
/// `~/.kaibo/config.toml`. Tests inject a fake implementation instead, so
/// config-resolution tests never read (or race on) the real process
/// environment - this trait is the injection point the hermetic-test
/// doctrine asks for.
trait Environment {
    fn var(&self, key: &str) -> Option<String>;
    fn home_dir(&self) -> Option<PathBuf>;
}

struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// Resolved, immutable configuration for a kaibo process.
///
/// Fields are private; read them through the getters below. There is no
/// setter and no public way to mutate a `Config` after it is built, by
/// design - pass it around as `&Config`, or wrap it in `Arc` to share it
/// across threads.
#[derive(Debug, Clone)]
pub struct Config {
    repo: Option<String>,
    clone: PathBuf,
    index: String,
    collection: String,
    api_url: Option<String>,
    sources: HashMap<ConfigKey, ConfigSource>,
}

impl Config {
    /// The only public constructor. Reads `KAIBO_*` environment variables
    /// and `~/.kaibo/config.toml`, in that order of precedence over
    /// compiled defaults. Call this exactly once, in `main`, before opening
    /// any corpus file.
    pub fn resolve() -> Result<Config, ConfigError> {
        Self::resolve_with(&ProcessEnvironment)
    }

    fn resolve_with(env: &dyn Environment) -> Result<Config, ConfigError> {
        let home = env.home_dir();
        let file = load_config_file(home.as_deref())?;

        let (repo, repo_source) =
            resolve_optional(env, ENV_REPO, file.as_ref().and_then(|f| f.repo.clone()));

        let (clone_raw, clone_source) = resolve_optional(
            env,
            ENV_CLONE,
            file.as_ref()
                .and_then(|f| f.clone.as_ref())
                .map(|p| p.to_string_lossy().into_owned()),
        );
        let (clone, clone_source) = match clone_raw {
            Some(value) => (PathBuf::from(value), clone_source),
            None => {
                let home = home.ok_or(ConfigError::NoHomeDir)?;
                (home.join(".kaibo").join("knowledge"), ConfigSource::Default)
            }
        };

        let (index, index_source) = resolve_with_default(
            env,
            ENV_INDEX,
            file.as_ref().and_then(|f| f.index.clone()),
            DEFAULT_INDEX,
        );
        let (collection, collection_source) = resolve_with_default(
            env,
            ENV_COLLECTION,
            file.as_ref().and_then(|f| f.collection.clone()),
            DEFAULT_COLLECTION,
        );
        let (api_url, api_url_source) = resolve_optional(
            env,
            ENV_API_URL,
            file.as_ref().and_then(|f| f.api_url.clone()),
        );

        let mut sources = HashMap::with_capacity(5);
        sources.insert(ConfigKey::Repo, repo_source);
        sources.insert(ConfigKey::Clone, clone_source);
        sources.insert(ConfigKey::Index, index_source);
        sources.insert(ConfigKey::Collection, collection_source);
        sources.insert(ConfigKey::ApiUrl, api_url_source);

        Ok(Config {
            repo,
            clone,
            index,
            collection,
            api_url,
            sources,
        })
    }

    /// The corpus git repo, `owner/name` form. `None` if unset: there is no
    /// sensible public default, so a verb that needs a repo reports that as
    /// its own error (e.g. pointing the user at `kaibo install`).
    pub fn repo(&self) -> Option<&str> {
        self.repo.as_deref()
    }

    /// Local path of the corpus clone.
    pub fn clone_path(&self) -> &Path {
        &self.clone
    }

    /// The qmd index name.
    pub fn index(&self) -> &str {
        &self.index
    }

    /// The qmd collection name.
    pub fn collection(&self) -> &str {
        &self.collection
    }

    /// The API URL to use instead of the local backend. `None` means use the
    /// local backend.
    pub fn api_url(&self) -> Option<&str> {
        self.api_url.as_deref()
    }

    /// Where the value for `key` came from: environment, file, or default.
    /// Not printed anywhere in this PR; a future `kaibo status` will use it.
    pub fn source(&self, key: ConfigKey) -> ConfigSource {
        *self
            .sources
            .get(&key)
            .expect("every ConfigKey is populated by Config::resolve_with")
    }
}

fn load_config_file(home: Option<&Path>) -> Result<Option<ConfigFile>, ConfigError> {
    let Some(home) = home else {
        return Ok(None);
    };
    let path = home.join(".kaibo").join("config.toml");
    if !path.is_file() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&path).map_err(|source| ConfigError::ReadFile {
        path: path.clone(),
        source,
    })?;
    let file = toml::from_str(&contents).map_err(|source| ConfigError::ParseFile {
        path,
        source: Box::new(source),
    })?;
    Ok(Some(file))
}

/// An empty value counts as unset, at every layer.
///
/// `std::env::var` cannot distinguish `KAIBO_REPO=` from a deliberate empty
/// string, and neither can a config file holding `repo = ""`. Treating empty
/// as unset means an exported-but-blank variable falls through to the next
/// layer instead of shadowing it with a value no verb could use - and it
/// keeps `repo()` returning `None` rather than `Some("")`, so the "unset
/// repo, point the user at `kaibo install`" path stays a single check.
fn resolve_optional(
    env: &dyn Environment,
    env_key: &str,
    file_value: Option<String>,
) -> (Option<String>, ConfigSource) {
    if let Some(value) = env.var(env_key).filter(|v| !v.is_empty()) {
        return (Some(value), ConfigSource::Env);
    }
    if let Some(value) = file_value.filter(|v| !v.is_empty()) {
        return (Some(value), ConfigSource::File);
    }
    (None, ConfigSource::Default)
}

fn resolve_with_default(
    env: &dyn Environment,
    env_key: &str,
    file_value: Option<String>,
    default: &str,
) -> (String, ConfigSource) {
    match resolve_optional(env, env_key, file_value) {
        (Some(value), source) => (value, source),
        (None, source) => (default.to_string(), source),
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Test-only: build a [`Config`] with explicit values, for tests in
    /// other modules that need one. Those tests cannot call
    /// [`Config::resolve_with`] (private to this module) and must not call
    /// [`Config::resolve`] (it would read the real environment and defeat
    /// the hermetic-test doctrine this module exists to uphold) - this is
    /// the sanctioned way for the rest of the crate to get a `Config` fixture.
    pub(crate) struct ConfigBuilder {
        repo: Option<String>,
        clone: PathBuf,
        index: String,
        collection: String,
        api_url: Option<String>,
        sources: HashMap<ConfigKey, ConfigSource>,
    }

    impl ConfigBuilder {
        pub(crate) fn new(clone: impl Into<PathBuf>) -> Self {
            let mut sources = HashMap::with_capacity(5);
            for key in [
                ConfigKey::Repo,
                ConfigKey::Clone,
                ConfigKey::Index,
                ConfigKey::Collection,
                ConfigKey::ApiUrl,
            ] {
                sources.insert(key, ConfigSource::Default);
            }
            Self {
                repo: None,
                clone: clone.into(),
                index: DEFAULT_INDEX.to_string(),
                collection: DEFAULT_COLLECTION.to_string(),
                api_url: None,
                sources,
            }
        }

        pub(crate) fn index(mut self, value: &str, source: ConfigSource) -> Self {
            self.index = value.to_string();
            self.sources.insert(ConfigKey::Index, source);
            self
        }

        pub(crate) fn collection(mut self, value: &str, source: ConfigSource) -> Self {
            self.collection = value.to_string();
            self.sources.insert(ConfigKey::Collection, source);
            self
        }

        pub(crate) fn repo(mut self, value: &str, source: ConfigSource) -> Self {
            self.repo = Some(value.to_string());
            self.sources.insert(ConfigKey::Repo, source);
            self
        }

        pub(crate) fn api_url(mut self, value: &str, source: ConfigSource) -> Self {
            self.api_url = Some(value.to_string());
            self.sources.insert(ConfigKey::ApiUrl, source);
            self
        }

        pub(crate) fn build(self) -> Config {
            Config {
                repo: self.repo,
                clone: self.clone,
                index: self.index,
                collection: self.collection,
                api_url: self.api_url,
                sources: self.sources,
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
}
