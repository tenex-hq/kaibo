//! Config resolution.
//!
//! Precedence, highest first: `KAIBO_*` environment variables, then
//! `~/.kaibo/config.toml`, then compiled defaults. [`LintConfig`]'s fields
//! are file-only (lists and maps, not scalars an env var carries cleanly),
//! so for those the precedence is just file then default.
//!
//! Config comes from configuration, never from content: [`Config::resolve`]
//! is the sole public constructor, and callers must resolve exactly once, in
//! `main`, before opening any corpus file - nothing read from the corpus can
//! reach a value used to decide where to read or write. This is what keeps
//! `lint.rs`'s rule parameters safe to read from a `[lint]` table: they are
//! bound here, in the same call, from the same two layers as `repo` or
//! `index`, and are just as unreachable from a page in the clone.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::error::{ExitCode, ExitCoded};

const ENV_REPO: &str = "KAIBO_REPO";
const ENV_CLONE: &str = "KAIBO_CLONE";
const ENV_INDEX: &str = "KAIBO_INDEX";
const ENV_COLLECTION: &str = "KAIBO_COLLECTION";
const ENV_API_URL: &str = "KAIBO_API_URL";
const ENV_NO_LOG: &str = "KAIBO_NO_LOG";
const ENV_OTLP_EXPORT: &str = "KAIBO_OTLP_EXPORT";

/// The OTLP export timeout, read from the standard variable but defaulted
/// low. The specification's own default is 10 seconds, which is the right
/// number for a long-lived service and the wrong one for a CLI an agent is
/// waiting on: a collector that has gone away would add ten seconds to every
/// `kaibo query`. An explicit `OTEL_EXPORTER_OTLP_TIMEOUT` still wins.
const ENV_OTEL_TIMEOUT: &str = "OTEL_EXPORTER_OTLP_TIMEOUT";
const DEFAULT_OTLP_TIMEOUT_MS: u64 = 2_000;

/// Where the paper trail is appended, under the `~/.kaibo/` workspace
/// ADR 0005 fixes. Not configurable: the trail is written by the binary for
/// the corpus's stewards, and a per-installation path would give a reader no
/// idea where to look.
const TRAIL_FILE: &str = "trail.jsonl";

/// Compiled default for `lint.frontmatter_contract.required_keys`: the four
/// fields `frontmatter-contract` has always required. Naming any of the
/// four here is what "required" means for it; a key with no dedicated
/// arm in [`crate::lint::rules::frontmatter_contract`] is still checked, only
/// generically (presence in the frontmatter's passthrough map).
const DEFAULT_LINT_REQUIRED_FRONTMATTER_KEYS: &[&str] = &["title", "tags", "status", "updated"];

/// Compiled default for `lint.tags_kebab_case.pattern`: lowercase ASCII
/// letters and digits, hyphen-separated, no leading, trailing or doubled
/// hyphen - the predicate `tags-kebab-case` has always enforced. Matched as
/// a full-string pattern (the rule anchors it), not a substring search.
const DEFAULT_LINT_TAG_PATTERN: &str = "[a-z0-9]+(-[a-z0-9]+)*";

/// Claude Code's own config-directory override. Read here rather than
/// guessed at, because `kaibo install` has to write where Claude Code
/// actually looks; a user who has moved that directory would otherwise get
/// a plugin installed into a path nothing reads.
const ENV_CLAUDE_CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";

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
    NoLog,
    OtlpExport,
    SkillsDir,
    LintDisabledRules,
    LintRequiredFrontmatterKeys,
    LintAllowedStatus,
    LintTypeFolderOverrides,
    LintTagPattern,
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
    no_log: Option<bool>,
    otlp_export: Option<bool>,
    lint: Option<LintConfigFile>,
}

/// The `[lint]` table: parameters for the compiled lint rules. See
/// [`crate::lint`] for why these five are the only surface - no rule file,
/// no shelled-out linter.
#[derive(Debug, Default, Deserialize)]
struct LintConfigFile {
    /// Rule ids to skip entirely, e.g. `["prose-style"]`. A rule id, not a
    /// path or a command - this can only ever remove a rule from the fixed
    /// set the registry already knows how to build, never name new code to
    /// run.
    disabled_rules: Option<Vec<String>>,
    frontmatter_contract: Option<FrontmatterContractConfigFile>,
    tags_kebab_case: Option<TagsKebabCaseConfigFile>,
}

#[derive(Debug, Default, Deserialize)]
struct FrontmatterContractConfigFile {
    required_keys: Option<Vec<String>>,
    allowed_status: Option<Vec<String>>,
    type_folder_overrides: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Default, Deserialize)]
struct TagsKebabCaseConfigFile {
    pattern: Option<String>,
}

/// Resolved parameters for the compiled lint rules - the whole surface of
/// [`ConfigKey::LintDisabledRules`] through [`ConfigKey::LintTagPattern`] in
/// one struct, mirroring how [`crate::lint::rules`] actually consumes it.
/// Every field defaults to the value each rule already hardcoded before this
/// existed, so a caller who configures nothing sees the exact old behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintConfig {
    /// Rule ids the registry skips. Empty means every rule runs, the same
    /// as before this field existed.
    pub disabled_rules: Vec<String>,
    /// Frontmatter keys `frontmatter-contract` requires. `title`, `tags`,
    /// `status` and `updated` are checked with their original messages;
    /// any other key is checked generically for presence in the
    /// frontmatter's passthrough map.
    pub required_frontmatter_keys: Vec<String>,
    /// The `status` values `frontmatter-contract` accepts. Empty means no
    /// restriction - any present `status` value passes, the original
    /// behaviour, since the rule only ever checked presence.
    pub allowed_status: Vec<String>,
    /// Folder name to expected `type` value, overriding the default
    /// identity mapping (a page under `reference/` is expected to say
    /// `type: reference`) for the folders named here. A folder not in this
    /// map keeps the identity default.
    pub type_folder_overrides: BTreeMap<String, String>,
    /// The pattern `tags-kebab-case` matches each tag against in full (the
    /// rule anchors it as `^(?:pattern)$`), as a `regex`-crate pattern.
    pub tag_pattern: String,
}

impl Default for LintConfig {
    fn default() -> Self {
        LintConfig {
            disabled_rules: Vec::new(),
            required_frontmatter_keys: DEFAULT_LINT_REQUIRED_FRONTMATTER_KEYS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            allowed_status: Vec::new(),
            type_folder_overrides: BTreeMap::new(),
            tag_pattern: DEFAULT_LINT_TAG_PATTERN.to_string(),
        }
    }
}

/// Where [`Config::resolve`] reads ambient state from. [`ProcessEnvironment`]
/// is the only implementation used outside tests and the only place in the
/// crate that calls `std::env::var` or reads `~/.kaibo/config.toml`; tests
/// inject a fake instead, so config-resolution tests never touch the real
/// process environment.
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

/// Where the wide event goes after the file, once export is switched on.
///
/// Carries no endpoint on purpose. Endpoint resolution is the OTel SDK's,
/// straight from `OTEL_EXPORTER_OTLP_ENDPOINT` and its signal-specific
/// sibling, so kaibo does not reimplement a spec it would only get subtly
/// wrong. What kaibo decides is *whether* to build an exporter at all, and
/// how long it may block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtlpTarget {
    pub timeout: Duration,
}

/// Resolved, immutable configuration for a kaibo process. Fields are
/// private with no setters; read them through the getters below.
#[derive(Debug, Clone)]
pub struct Config {
    repo: Option<String>,
    clone: PathBuf,
    index: String,
    collection: String,
    api_url: Option<String>,
    no_log: bool,
    trail: Option<PathBuf>,
    otlp_export: bool,
    otlp_timeout: Duration,
    skills_dir: Option<PathBuf>,
    lint: LintConfig,
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

        // `None` rather than an error, because only `install` needs this:
        // a machine with no home directory and an explicit `KAIBO_CLONE`
        // can still run every reading verb, and failing resolution here
        // would take those down with it.
        let (skills_dir, skills_dir_source) =
            match env.var(ENV_CLAUDE_CONFIG_DIR).filter(|v| !v.is_empty()) {
                Some(dir) => (Some(PathBuf::from(dir).join("skills")), ConfigSource::Env),
                None => (
                    home.as_ref().map(|h| h.join(".claude").join("skills")),
                    ConfigSource::Default,
                ),
            };

        let (no_log, no_log_source) =
            resolve_bool(env, ENV_NO_LOG, file.as_ref().and_then(|f| f.no_log));

        // Derived from `home`, never from `clone`: the two are siblings under
        // `~/.kaibo`, and an explicit `KAIBO_CLONE` pointing at a checkout
        // elsewhere must not scatter trail files beside it. `None` when there
        // is no home at all, which is the one case the reading verbs still
        // have to survive.
        let trail = home.as_ref().map(|h| h.join(".kaibo").join(TRAIL_FILE));

        let (otlp_export, otlp_export_source) = resolve_bool(
            env,
            ENV_OTLP_EXPORT,
            file.as_ref().and_then(|f| f.otlp_export),
        );
        let otlp_timeout = Duration::from_millis(
            env.var(ENV_OTEL_TIMEOUT)
                .and_then(|raw| raw.parse::<u64>().ok())
                .filter(|ms| *ms > 0)
                .unwrap_or(DEFAULT_OTLP_TIMEOUT_MS),
        );

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

        let lint_file = file.as_ref().and_then(|f| f.lint.as_ref());
        let fm_contract_file = lint_file.and_then(|l| l.frontmatter_contract.as_ref());
        let tags_file = lint_file.and_then(|l| l.tags_kebab_case.as_ref());

        // File-only, no `KAIBO_*` override: these are lists and maps, not
        // scalars an environment variable can carry cleanly, so the file is
        // the only layer above the compiled default. Unlike the string
        // helpers above, an empty list *is* a distinct, meaningful choice a
        // config file can make (e.g. `allowed_status = []` staying explicit
        // about "no restriction"), so presence of the key - not emptiness -
        // is what marks a value as coming from the file: TOML already tells
        // absent from empty apart, so there is no `KAIBO_REPO=""` ambiguity
        // to guard against here.
        let (disabled_rules, disabled_rules_source) =
            resolve_file_only(lint_file.and_then(|l| l.disabled_rules.clone()), Vec::new());
        let (required_frontmatter_keys, required_frontmatter_keys_source) = resolve_file_only(
            fm_contract_file.and_then(|f| f.required_keys.clone()),
            DEFAULT_LINT_REQUIRED_FRONTMATTER_KEYS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        let (allowed_status, allowed_status_source) = resolve_file_only(
            fm_contract_file.and_then(|f| f.allowed_status.clone()),
            Vec::new(),
        );
        let (type_folder_overrides, type_folder_overrides_source) = resolve_file_only(
            fm_contract_file.and_then(|f| f.type_folder_overrides.clone()),
            BTreeMap::new(),
        );
        let (tag_pattern, tag_pattern_source) = match tags_file.and_then(|t| t.pattern.clone()) {
            Some(pattern) => (pattern, ConfigSource::File),
            None => (DEFAULT_LINT_TAG_PATTERN.to_string(), ConfigSource::Default),
        };

        let lint = LintConfig {
            disabled_rules,
            required_frontmatter_keys,
            allowed_status,
            type_folder_overrides,
            tag_pattern,
        };

        let mut sources = HashMap::with_capacity(13);
        sources.insert(ConfigKey::Repo, repo_source);
        sources.insert(ConfigKey::Clone, clone_source);
        sources.insert(ConfigKey::Index, index_source);
        sources.insert(ConfigKey::Collection, collection_source);
        sources.insert(ConfigKey::ApiUrl, api_url_source);
        sources.insert(ConfigKey::NoLog, no_log_source);
        sources.insert(ConfigKey::OtlpExport, otlp_export_source);
        sources.insert(ConfigKey::SkillsDir, skills_dir_source);
        sources.insert(ConfigKey::LintDisabledRules, disabled_rules_source);
        sources.insert(
            ConfigKey::LintRequiredFrontmatterKeys,
            required_frontmatter_keys_source,
        );
        sources.insert(ConfigKey::LintAllowedStatus, allowed_status_source);
        sources.insert(
            ConfigKey::LintTypeFolderOverrides,
            type_folder_overrides_source,
        );
        sources.insert(ConfigKey::LintTagPattern, tag_pattern_source);

        Ok(Config {
            repo,
            clone,
            index,
            collection,
            api_url,
            no_log,
            trail,
            otlp_export,
            otlp_timeout,
            skills_dir,
            lint,
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

    /// Where to append the paper trail, or `None` when nothing should be
    /// written: either `no_log` is set, or there is no home directory to
    /// resolve `~/.kaibo` against.
    ///
    /// One accessor rather than a path plus a flag, so a caller cannot
    /// consult the path and forget the switch.
    pub fn trail_path(&self) -> Option<&Path> {
        if self.no_log {
            return None;
        }
        self.trail.as_deref()
    }

    /// Where to push the same event after the JSONL line is written, or
    /// `None` when nothing should leave the machine.
    ///
    /// The gate is kaibo's own key and nothing else. `OTEL_EXPORTER_OTLP_*`
    /// is honoured for conformance once export is on, but an endpoint in the
    /// ambient environment must never by itself start sending: that variable
    /// is commonly exported machine-wide, and the event carries the question
    /// someone asked. Inheriting a network target ambiently is exactly what
    /// resolving config in one place exists to prevent.
    pub fn otlp(&self) -> Option<OtlpTarget> {
        if !self.otlp_export {
            return None;
        }
        Some(OtlpTarget {
            timeout: self.otlp_timeout,
        })
    }

    /// The directory Claude Code discovers plugins in:
    /// `<CLAUDE_CONFIG_DIR>/skills` when that variable is set and non-empty,
    /// otherwise `~/.claude/skills`. `None` when neither is available, which
    /// only `kaibo install` has to care about.
    pub fn skills_dir(&self) -> Option<&Path> {
        self.skills_dir.as_deref()
    }

    /// Parameters for the compiled lint rules. See [`LintConfig`] and
    /// [`crate::lint`] for what each field governs and why this is the only
    /// way a rule's behaviour can change.
    pub fn lint(&self) -> &LintConfig {
        &self.lint
    }

    /// Where the value for `key` came from: environment, file, or default.
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

/// An empty value counts as unset, at every layer: `std::env::var` cannot
/// distinguish `KAIBO_REPO=` from a deliberate empty string, and neither can
/// a config file holding `repo = ""`. An exported-but-blank variable falls
/// through to the next layer rather than shadowing it with a value no verb
/// could use.
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

/// File-only resolution for a config value with no environment-variable
/// layer (a list or a map, not a scalar `KAIBO_*` can carry): the file's
/// value if the key was present at all, the compiled default otherwise.
fn resolve_file_only<T>(file_value: Option<T>, default: T) -> (T, ConfigSource) {
    match file_value {
        Some(value) => (value, ConfigSource::File),
        None => (default, ConfigSource::Default),
    }
}

/// Boolean resolution, same three layers as [`resolve_optional`]. A value
/// that is neither a recognised yes nor a recognised no falls through to the
/// next layer rather than being read as either: `KAIBO_NO_LOG=maybe` is a
/// typo, and taking any non-empty string as "on" would let one silently stop
/// the paper trail for good. `0` is a deliberate no, not an empty-ish value,
/// which is why this cannot reuse the string helper.
fn resolve_bool(
    env: &dyn Environment,
    env_key: &str,
    file_value: Option<bool>,
) -> (bool, ConfigSource) {
    if let Some(value) = env.var(env_key).as_deref().and_then(parse_bool) {
        return (value, ConfigSource::Env);
    }
    if let Some(value) = file_value {
        return (value, ConfigSource::File);
    }
    (false, ConfigSource::Default)
}

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
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
    /// other modules. Cannot call [`Config::resolve_with`] (private) or
    /// [`Config::resolve`] (reads the real environment) - this is the
    /// sanctioned way to get a `Config` fixture.
    pub(crate) struct ConfigBuilder {
        repo: Option<String>,
        clone: PathBuf,
        index: String,
        collection: String,
        api_url: Option<String>,
        no_log: bool,
        trail: Option<PathBuf>,
        otlp_export: bool,
        otlp_timeout: Duration,
        skills_dir: Option<PathBuf>,
        lint: LintConfig,
        sources: HashMap<ConfigKey, ConfigSource>,
    }

    impl ConfigBuilder {
        pub(crate) fn new(clone: impl Into<PathBuf>) -> Self {
            let mut sources = HashMap::with_capacity(13);
            for key in [
                ConfigKey::Repo,
                ConfigKey::Clone,
                ConfigKey::Index,
                ConfigKey::Collection,
                ConfigKey::ApiUrl,
                ConfigKey::NoLog,
                ConfigKey::OtlpExport,
                ConfigKey::SkillsDir,
                ConfigKey::LintDisabledRules,
                ConfigKey::LintRequiredFrontmatterKeys,
                ConfigKey::LintAllowedStatus,
                ConfigKey::LintTypeFolderOverrides,
                ConfigKey::LintTagPattern,
            ] {
                sources.insert(key, ConfigSource::Default);
            }
            Self {
                // No install location unless a test asks for one: a
                // fixture that silently pointed `install` somewhere would
                // have it writing into another test's corpus.
                skills_dir: None,
                clone: clone.into(),
                repo: None,
                index: DEFAULT_INDEX.to_string(),
                collection: DEFAULT_COLLECTION.to_string(),
                api_url: None,
                // No trail unless a test asks for one, for the same reason
                // as `skills_dir`: a fixture that silently wrote somewhere
                // would be writing during every other module's tests.
                no_log: false,
                trail: None,
                otlp_export: false,
                otlp_timeout: Duration::from_millis(DEFAULT_OTLP_TIMEOUT_MS),
                lint: LintConfig::default(),
                sources,
            }
        }

        pub(crate) fn skills_dir(mut self, value: impl Into<PathBuf>) -> Self {
            self.skills_dir = Some(value.into());
            self.sources.insert(ConfigKey::SkillsDir, ConfigSource::Env);
            self
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

        pub(crate) fn lint_disabled_rules(
            mut self,
            value: Vec<String>,
            source: ConfigSource,
        ) -> Self {
            self.lint.disabled_rules = value;
            self.sources.insert(ConfigKey::LintDisabledRules, source);
            self
        }

        pub(crate) fn lint_required_frontmatter_keys(
            mut self,
            value: Vec<String>,
            source: ConfigSource,
        ) -> Self {
            self.lint.required_frontmatter_keys = value;
            self.sources
                .insert(ConfigKey::LintRequiredFrontmatterKeys, source);
            self
        }

        pub(crate) fn lint_allowed_status(
            mut self,
            value: Vec<String>,
            source: ConfigSource,
        ) -> Self {
            self.lint.allowed_status = value;
            self.sources.insert(ConfigKey::LintAllowedStatus, source);
            self
        }

        pub(crate) fn lint_type_folder_overrides(
            mut self,
            value: BTreeMap<String, String>,
            source: ConfigSource,
        ) -> Self {
            self.lint.type_folder_overrides = value;
            self.sources
                .insert(ConfigKey::LintTypeFolderOverrides, source);
            self
        }

        pub(crate) fn lint_tag_pattern(mut self, value: &str, source: ConfigSource) -> Self {
            self.lint.tag_pattern = value.to_string();
            self.sources.insert(ConfigKey::LintTagPattern, source);
            self
        }

        pub(crate) fn build(self) -> Config {
            Config {
                repo: self.repo,
                clone: self.clone,
                index: self.index,
                collection: self.collection,
                api_url: self.api_url,
                no_log: self.no_log,
                trail: self.trail,
                otlp_export: self.otlp_export,
                otlp_timeout: self.otlp_timeout,
                skills_dir: self.skills_dir,
                lint: self.lint,
                sources: self.sources,
            }
        }
    }
}

#[cfg(test)]
mod tests;
