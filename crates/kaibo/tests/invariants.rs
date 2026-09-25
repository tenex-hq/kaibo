//! Structural invariants that hold across the workspace rather than inside one
//! module: each is easy to regress in a refactor and invisible to behaviour
//! tests, so each is held by a sweep over the source instead.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

/// Every `.rs` file under each crate's `src/`, test modules included. The
/// integration tests under `crates/*/tests/` are not swept: they drive the
/// binary from outside and may inspect the real machine to prove isolation.
fn crate_sources(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read directory") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    for krate in std::fs::read_dir(root.join("crates")).expect("read crates/") {
        let src = krate.expect("crate entry").path().join("src");
        if src.is_dir() {
            walk(&src, &mut files);
        }
    }
    assert!(
        !files.is_empty(),
        "the sweep found no source files under {}",
        root.display()
    );
    files
}

/// Files whose code, comments aside, contains `needle`, relative to `root`.
fn files_with_code_containing(root: &Path, needle: &str) -> Vec<String> {
    crate_sources(root)
        .into_iter()
        .filter(|path| {
            std::fs::read_to_string(path)
                .expect("read source file")
                .lines()
                .any(|line| !line.trim_start().starts_with("//") && line.contains(needle))
        })
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string()
        })
        .collect()
}

/// `Config::resolve` reads the environment through the injectable
/// `Environment` trait, which is what lets every test run without mutating
/// the real process environment. A second live read anywhere else is a config
/// value nothing resolves, and an untestable one. The qmd contract check is
/// the one carve-out, documented in its module doc: it locates qmd's own
/// state, not kaibo configuration.
#[test]
fn only_config_reads_the_process_environment() {
    let root = workspace_root();
    let allowed = [
        "crates/kaibo-core/src/config.rs",
        "crates/kaibo-core/src/status/qmd_contract_check.rs",
    ];

    let mut violations: Vec<String> = ["std::env", "env::var"]
        .into_iter()
        .flat_map(|needle| files_with_code_containing(&root, needle))
        .filter(|file| !allowed.contains(&file.as_str()))
        .collect();
    violations.sort();
    violations.dedup();

    assert!(
        violations.is_empty(),
        "the process environment is read outside Config::resolve in {violations:?}; \
         add the value to Config and read it through the Environment trait"
    );
}

/// The exit code is a contract (`0`/`1`/`2`/`3`/`4`), defined once as
/// `kaibo_core::error::ExitCode` and turned into a process exit in exactly one
/// place, `main.rs`'s `to_process_exit_code`. A `process::exit` elsewhere skips
/// that mapping, and skips every destructor on the way out.
#[test]
fn the_process_exit_code_is_produced_in_one_place() {
    let root = workspace_root();

    let exits = files_with_code_containing(&root, "process::exit");
    assert!(
        exits.is_empty(),
        "std::process::exit is called in {exits:?}; return an ExitCoded error to main instead"
    );

    let conversions = files_with_code_containing(&root, "ExitCode::from(");
    assert_eq!(
        conversions,
        ["crates/kaibo/src/main.rs"],
        "a process exit code is built outside main.rs's to_process_exit_code"
    );
}

/// kaibo-core's errors are `thiserror` enums, one variant per situation, each
/// message naming the command that fixes it and each mapped to an exit code.
/// A type-erased error loses both the exit code and the instruction.
#[test]
fn the_core_types_its_errors_rather_than_erasing_them() {
    let manifest = std::fs::read_to_string(workspace_root().join("crates/kaibo-core/Cargo.toml"))
        .expect("read kaibo-core's Cargo.toml");

    for erased in ["anyhow", "eyre"] {
        assert!(
            !manifest.contains(erased),
            "kaibo-core depends on {erased}; add a thiserror variant that implements ExitCoded instead"
        );
    }
}

/// `rust-toolchain.toml` pins the toolchain locally, and CI installs it
/// separately. The two drifting apart means CI lints and tests with a
/// compiler nobody runs locally.
#[test]
fn ci_installs_the_toolchain_rust_toolchain_toml_pins() {
    let root = workspace_root();
    let toolchain = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        .expect("read rust-toolchain.toml");
    let channel = toolchain
        .lines()
        .find_map(|line| line.trim().strip_prefix("channel = "))
        .expect("rust-toolchain.toml pins a channel")
        .trim_matches('"');

    let mut installs = Vec::new();
    for workflow in std::fs::read_dir(root.join(".github/workflows")).expect("read workflows") {
        let path = workflow.expect("workflow entry").path();
        let contents = std::fs::read_to_string(&path).expect("read workflow");
        for line in contents.lines() {
            if let Some((_, pin)) = line.split_once("dtolnay/rust-toolchain@") {
                installs.push((
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    pin.trim().to_string(),
                ));
            }
        }
    }

    assert!(
        !installs.is_empty(),
        "no workflow installs a Rust toolchain"
    );
    for (workflow, pin) in installs {
        assert_eq!(
            pin, channel,
            "{workflow} installs Rust {pin}, rust-toolchain.toml pins {channel}; change both together"
        );
    }
}
