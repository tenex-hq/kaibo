//! Opt-in, non-hermetic smoke check against a *real* `qmd` binary.
//!
//! Every other test in this crate is hermetic: no network, no subprocess, no
//! filesystem mutation outside a `tempfile` directory (see
//! `crate::process::testing::FakeCommandRunner`). This module is the
//! deliberate exception. It shells out to whatever `qmd` is on `PATH` and
//! reads/writes real files under the developer's `$HOME` (or `$XDG_*`), to
//! check three properties of qmd's own behaviour that a fake runner cannot
//! stand in for - they only break when qmd itself changes:
//!
//! (a) **Collection isolation.** `qmd collection add --index <scratch>`
//!     must not leak into qmd's default index: `qmd collection list`
//!     against the default index is byte-identical before and after.
//! (b) **Mask semantics.** The mask `*/{reference,how-to,faq}/**/*.md` must
//!     match a domain-nested page (`<domain>/<type>/<page>.md`) and exclude
//!     a file at the fixture root - checked indirectly through `qmd
//!     status`'s file count, since no [`QmdCommand`] builder exists for
//!     `qmd ls` (see the note on
//!     [`assert_mask_accepts_nested_and_rejects_root`]).
//! (c) **Write isolation.** `qmd update --index <scratch>` must leave the
//!     default index's sqlite database untouched - its mtime does not
//!     change.
//!
//! Because it is not hermetic, it is gated behind an environment variable
//! and skips cleanly - printing why - unless a developer opts in:
//!
//! ```text
//! KAIBO_QMD_CONTRACT=1 cargo test -p kaibo-core qmd_contract_check -- --nocapture
//! ```
//!
//! Two deliberate distinctions from `crate::config`'s "config comes from
//! `Config` only" rule, both worth calling out explicitly since this is the
//! one module in the crate that reads `std::env` outside that module:
//!
//! - [`GATE_VAR`] is a *test gate*, not application configuration. It never
//!   reaches a [`Config`], and reading it here does not reopen the
//!   guarantee `crate::config` documents.
//! - `$HOME` / `$XDG_CACHE_HOME` / `$XDG_CONFIG_HOME` below locate *qmd's
//!   own* on-disk state - the default index this check must never write
//!   to, and the scratch index it must clean up after itself - not kaibo
//!   configuration. Kaibo has no config key for "where qmd keeps its
//!   databases" and should not grow one just for this check.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::config::ConfigSource;
use crate::config::testing::ConfigBuilder;
use crate::explain::PlannedCommand;
use crate::process::{CommandRunner, RealCommandRunner};
use crate::qmd::QmdCommand;
use crate::status::IndexStatus;

/// The environment variable that opts into this check. Read directly with
/// `std::env::var`, deliberately not through `Config` - see the module doc.
const GATE_VAR: &str = "KAIBO_QMD_CONTRACT";

/// Name of the throwaway index this check creates and tears down. Chosen to
/// be obviously not a real user index; [`ScratchIndexPaths::cleanup`] always
/// removes every file it produces, including leftovers from a previous
/// failed run.
const SCRATCH_INDEX: &str = "kaibo-contract-check";

/// The mask kaibo's own `sync` verb registers its collection with. Kept as
/// a local literal rather than imported from `crate::sync`, whose
/// `COLLECTION_MASK` is private to that module - this is a fixture input
/// for this check, not a shared production constant.
const MASK: &str = "*/{reference,how-to,faq}/**/*.md";

/// A fixture's worth of markdown, laid out exactly like the mask
/// `MASK` is meant to accept or reject: one page nested two levels under
/// the fixture root (`<domain>/<type>/<page>.md`, matching), and one file
/// sitting at the fixture root (`README.md`, must be excluded).
fn write_fixture(root: &std::path::Path) {
    let nested_dir = root.join("kaibo").join("reference");
    std::fs::create_dir_all(&nested_dir).expect("create fixture nested dir");
    std::fs::write(
        nested_dir.join("fixture.md"),
        "---\ntype: reference\ntitle: Fixture\n---\nfixture body\n",
    )
    .expect("write fixture nested page");
    std::fs::write(root.join("README.md"), "root file, must not be indexed\n")
        .expect("write fixture root file");
}

/// Where qmd keeps the default index and the scratch index this check
/// creates, mirroring the shell contract check's path derivation exactly:
/// `${XDG_CACHE_HOME:-$HOME/.cache}/qmd/<name>.sqlite` for sqlite databases,
/// `${XDG_CONFIG_HOME:-$HOME/.config}/qmd/<name>.yml` for collection
/// definitions (the sqlite is derived from those; deleting only the sqlite
/// leaves an orphaned definition behind).
struct ScratchIndexPaths {
    default_db: PathBuf,
    scratch_db: PathBuf,
    scratch_wal: PathBuf,
    scratch_shm: PathBuf,
    scratch_yml: PathBuf,
}

impl ScratchIndexPaths {
    fn resolve() -> Option<Self> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let cache_home = non_empty_env("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"));
        let config_home = non_empty_env("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));

        Some(Self {
            default_db: cache_home.join("qmd").join("index.sqlite"),
            scratch_db: cache_home
                .join("qmd")
                .join(format!("{SCRATCH_INDEX}.sqlite")),
            scratch_wal: cache_home
                .join("qmd")
                .join(format!("{SCRATCH_INDEX}.sqlite-wal")),
            scratch_shm: cache_home
                .join("qmd")
                .join(format!("{SCRATCH_INDEX}.sqlite-shm")),
            scratch_yml: config_home.join("qmd").join(format!("{SCRATCH_INDEX}.yml")),
        })
    }

    /// Remove every scratch-index file, ignoring "already gone". Called
    /// both before the run (clearing leftovers from a previous failed run,
    /// like the shell check's `cleanup 2>/dev/null || true`) and after, via
    /// `Drop` - the same role the shell check's `trap cleanup EXIT` plays,
    /// except this one also runs on a panic, not only a clean exit.
    fn cleanup(&self) {
        for path in [
            &self.scratch_db,
            &self.scratch_wal,
            &self.scratch_shm,
            &self.scratch_yml,
        ] {
            if let Err(err) = std::fs::remove_file(path)
                && err.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "qmd contract check: could not remove scratch file {}: {err}",
                    path.display()
                );
            }
        }
    }

    fn default_db_mtime(&self) -> SystemTime {
        std::fs::metadata(&self.default_db)
            .unwrap_or_else(|err| {
                panic!(
                    "qmd contract check: could not stat the default qmd index database at {} \
                     ({err}) - this check assumes it already exists; run any real `qmd` \
                     command against your default index at least once, then re-run this check",
                    self.default_db.display()
                )
            })
            .modified()
            .unwrap_or_else(|err| {
                panic!(
                    "qmd contract check: platform does not support mtime for {}: {err}",
                    self.default_db.display()
                )
            })
    }
}

impl Drop for ScratchIndexPaths {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Run `command` against the real `qmd` binary and return its stdout,
/// panicking with an actionable message - naming the command that failed
/// and why - if it could not be spawned or exited non-zero.
fn run_ok(runner: &dyn CommandRunner, command: &PlannedCommand) -> String {
    match runner.run(command) {
        Ok(output) if output.success() => output.stdout,
        Ok(output) => panic!(
            "qmd contract check: `{command}` exited non-zero: {}",
            output.stderr.trim()
        ),
        Err(err) => panic!(
            "qmd contract check: could not run `{command}`: {err} - is qmd on PATH? \
             (this check requires a real qmd binary, see docs on KAIBO_QMD_CONTRACT)"
        ),
    }
}

#[test]
fn qmd_contract_check() {
    // `non_empty_env`, not `env::var(..).is_err()`: an exported-but-blank
    // `KAIBO_QMD_CONTRACT=` counts as unset here, the same rule `Config`
    // applies to every value it resolves. The fail-safe direction matters -
    // a blank variable must not arm a test that shells out to a real binary
    // and touches real state under $HOME.
    if non_empty_env(GATE_VAR).is_none() {
        println!(
            "qmd contract check: skipped (this test is non-hermetic - it runs a real qmd \
             binary and reads/writes real qmd state under $HOME). Set {GATE_VAR}=1 to run it: \
             `{GATE_VAR}=1 cargo test -p kaibo-core qmd_contract_check -- --nocapture`"
        );
        return;
    }

    let paths = ScratchIndexPaths::resolve().unwrap_or_else(|| {
        panic!(
            "qmd contract check: could not determine $HOME, needed to locate qmd's state \
             directories"
        )
    });
    // Clear any leftovers from a previous failed run before touching anything,
    // mirroring the shell check's `cleanup 2>/dev/null || true`.
    paths.cleanup();

    let fixture = tempfile::tempdir().expect("create fixture tempdir");
    write_fixture(fixture.path());

    let runner = RealCommandRunner;
    let config = ConfigBuilder::new(fixture.path())
        .index(SCRATCH_INDEX, ConfigSource::File)
        .build();

    let default_before = run_ok(&runner, &QmdCommand::default_index_collection_list());
    let default_db_mtime_before = paths.default_db_mtime();

    run_ok(
        &runner,
        &QmdCommand::collection_add(&config, fixture.path(), "knowledge", MASK),
    );
    run_ok(&runner, &QmdCommand::update(&config));

    assert_isolation_untouched(&runner, &default_before);
    assert_mask_accepts_nested_and_rejects_root(&runner, &paths, &config);
    assert_default_db_untouched(&paths, default_db_mtime_before);

    let version = run_ok(&runner, &QmdCommand::version());
    println!("qmd contract check: all green ({})", version.trim());
}

/// (a) Collection isolation: adding a collection to the scratch index must
/// not change what `qmd collection list` reports for the *default* index.
fn assert_isolation_untouched(runner: &dyn CommandRunner, default_before: &str) {
    let default_after = run_ok(runner, &QmdCommand::default_index_collection_list());
    assert_eq!(
        default_before, default_after,
        "qmd contract broken: `qmd collection add --index <scratch>` changed the DEFAULT \
         index's `qmd collection list` output. --index no longer isolates collections between \
         indexes the way kaibo's index-split design assumes (see docs on qmd's index \
         semantics) - a qmd upgrade likely changed how --index scopes collection state.\n\
         before:\n{default_before}\nafter:\n{default_after}"
    );
}

/// (b) Mask semantics. No [`QmdCommand`] builder exists for `qmd ls`, so
/// this cannot list matched files by name the way the shell contract check
/// does; all it can read is `qmd status --index <scratch>`'s file count.
///
/// A count alone is not enough. Indexing the mixed fixture (one
/// domain-nested page that should match, one root-level `README.md` that
/// should not) and asserting `1` would also pass if the mask had
/// *inverted*, matching the root file and rejecting the nested one. The
/// count cannot distinguish those, so the assertion is made twice against
/// different fixtures:
///
/// - mixed fixture, expect exactly 1: something matched, something did not.
///
/// - root-only fixture, expect exactly 0: what did not match is the root
///   file specifically.
///
///
/// Together those pin the mask's direction, which one count cannot. This is
/// still weaker than naming the matched file: a mask that matched some
/// third path shape would satisfy both. Replace this with a `qmd ls`
/// builder if one is ever added.
fn assert_mask_accepts_nested_and_rejects_root(
    runner: &dyn CommandRunner,
    paths: &ScratchIndexPaths,
    mixed_config: &crate::config::Config,
) {
    assert_indexed_file_count(
        runner,
        mixed_config,
        1,
        "the mixed fixture (domain-nested `kaibo/reference/fixture.md` must match; root-level \
         `README.md` must not)",
    );

    // Second fixture, own scratch index: clear the first one first, or
    // `collection add` would fail on the already-registered name.
    paths.cleanup();
    let root_only = tempfile::tempdir().expect("create root-only fixture tempdir");
    std::fs::write(
        root_only.path().join("README.md"),
        "root file, must not be indexed\n",
    )
    .expect("write root-only fixture file");

    let root_only_config = ConfigBuilder::new(root_only.path())
        .index(SCRATCH_INDEX, ConfigSource::File)
        .build();
    run_ok(
        runner,
        &QmdCommand::collection_add(&root_only_config, root_only.path(), "knowledge", MASK),
    );
    run_ok(runner, &QmdCommand::update(&root_only_config));

    assert_indexed_file_count(
        runner,
        &root_only_config,
        0,
        "the root-only fixture (a bare `README.md` at the collection root must match nothing)",
    );
}

fn assert_indexed_file_count(
    runner: &dyn CommandRunner,
    config: &crate::config::Config,
    expected: u64,
    fixture_description: &str,
) {
    let status_output = run_ok(runner, &QmdCommand::status(config));
    let status = crate::status::parse_qmd_status(&status_output);
    match status {
        IndexStatus::Available { total_files, .. } if total_files == Some(expected) => {}
        other => panic!(
            "qmd contract broken: expected exactly {expected} file(s) indexed for \
             {fixture_description}, got {other:?} instead. The mask \
             `*/{{reference,how-to,faq}}/**/*.md` no longer matches \
             `<domain>/{{reference,how-to,faq}}/**/*.md` and excludes root files the way \
             kaibo's `sync` verb (and the docs describing qmd's mask semantics) assume - a qmd \
             upgrade likely changed glob matching.\nraw `qmd status` output:\n{status_output}"
        ),
    }
}

/// (c) Write isolation: `qmd update --index <scratch>` must leave the
/// default index's sqlite database mtime unchanged.
fn assert_default_db_untouched(paths: &ScratchIndexPaths, mtime_before: SystemTime) {
    let mtime_after = paths.default_db_mtime();
    assert_eq!(
        mtime_before,
        mtime_after,
        "qmd contract broken: `qmd update --index <scratch>` changed the mtime of the DEFAULT \
         index's sqlite database at {} - qmd update no longer scopes strictly per-index, and a \
         call scoped to kaibo's own index could now write into a user's personal default index \
         too. A qmd upgrade likely changed update's write scope.",
        paths.default_db.display()
    );
}
