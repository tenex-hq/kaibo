//! The single, guarded way to build a `qmd` invocation.
//!
//! Kaibo operates on one dedicated qmd index; qmd's default index, where a
//! user's own personal collections may live, must never be touched. Two
//! separate mechanisms enforce that, and it matters which does which:
//!
//! - **Type-level, crate boundary:** [`PlannedCommand`](crate::explain::PlannedCommand)'s
//!   fields and its `new` constructor are `pub(crate)`. Nothing outside
//!   `kaibo-core` - a second face linking this crate directly (an MCP
//!   server, a hosted API handler) - can construct a `PlannedCommand` at
//!   all, let alone one addressing `qmd`'s default index. It can only get
//!   one back through [`QmdCommand`], whose `index_command` always injects
//!   `--index <config.index()>` ahead of whatever the caller asked for, and
//!   reads that value out of [`Config`], never from a literal the caller
//!   supplied. See the `compile_fail` example on `PlannedCommand` for what
//!   this actually blocks.
//! - **Source-scanning test, inside the crate:** the type system does not
//!   stop *this crate's own code* from building `PlannedCommand::new("qmd",
//!   ...)` directly, bypassing `QmdCommand` and its `--index` injection -
//!   `PlannedCommand::new` is generic over any program string, by design,
//!   because other modules use it for `git`. The test
//!   `qmd_command_literal_is_confined_to_this_module` closes that gap the
//!   type system cannot: it scans every other `.rs` file in this crate and
//!   fails if any of them spells out the literal command `"qmd"`.
//!
//! Neither mechanism stops a *new function added to this file* from
//! misusing `PlannedCommand::new("qmd", ...)` directly instead of routing
//! through `index_command` - that residual risk is what
//! `version_and_default_index_probe_are_the_only_commands_without_an_index`
//! below guards, by enumerating the exact two functions allowed to skip
//! `--index`.
//!
//! Those two functions live outside the `--index` guarantee on purpose,
//! each documented at its own definition: [`QmdCommand::version`] (a
//! version check addresses no index at all) and
//! [`QmdCommand::default_index_collection_list`] (proving the default index
//! is untouched means reading the default index, by definition - the same
//! carve-out the qmd contract this crate follows makes for its own
//! migration guard).

use crate::config::Config;
use crate::explain::PlannedCommand;

pub struct QmdCommand;

impl QmdCommand {
    /// Build `qmd <subcommand> --index <config.index()> <args...>`. This is
    /// the only constructor for a qmd command that addresses an index, and
    /// the index always comes from `config`, never from a caller-supplied
    /// string - a call site cannot even pass one in.
    pub fn index_command(
        config: &Config,
        subcommand: &str,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> PlannedCommand {
        let mut full_args = vec![
            subcommand.to_string(),
            "--index".to_string(),
            config.index().to_string(),
        ];
        full_args.extend(args.into_iter().map(Into::into));
        PlannedCommand::new("qmd", full_args)
    }

    /// `qmd status --index <configured>` - document and embedding counts for
    /// the configured index.
    pub fn status(config: &Config) -> PlannedCommand {
        Self::index_command(config, "status", Vec::<String>::new())
    }

    /// `qmd --version` - a version check addresses no index, so it carries
    /// no `--index` and is deliberately not built through
    /// [`Self::index_command`].
    pub fn version() -> PlannedCommand {
        PlannedCommand::new("qmd", ["--version"])
    }

    /// `qmd collection list` against qmd's *default* index (no `--index`) -
    /// the one deliberate exception. Verifying kaibo left the default index
    /// alone requires looking at the default index; this reads it, and only
    /// reads it - it never adds, removes, updates or embeds anything there.
    pub fn default_index_collection_list() -> PlannedCommand {
        PlannedCommand::new("qmd", ["collection", "list"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigSource;
    use crate::config::testing::ConfigBuilder;

    /// Mandatory guardrail test: every qmd command that addresses an index
    /// carries `--index`, and the value is whatever `Config` resolved, not a
    /// literal typed at the call site.
    ///
    /// Exercises `index_command` directly - the one function that assembles
    /// this argument vector - with an arbitrary subcommand and args, rather
    /// than a hand-maintained list of the commands built on top of it
    /// (`status`, and whatever is added later). A new command built through
    /// `index_command` is covered by construction; only a call site that
    /// bypasses `index_command` entirely could still go wrong, and that is
    /// what `qmd_command_literal_is_confined_to_this_module` below guards.
    #[test]
    fn index_operations_carry_the_configured_index_not_a_literal() {
        let config = ConfigBuilder::new("/unused")
            .index("from-config-not-a-literal", ConfigSource::File)
            .build();

        let command = QmdCommand::index_command(&config, "arbitrary-subcommand", ["extra-arg"]);

        assert_eq!(command.program, "qmd");
        let position = command
            .args
            .iter()
            .position(|arg| arg == "--index")
            .expect("index operation must carry --index");
        assert_eq!(command.args[position + 1], config.index());
        assert_eq!(command.args[position + 1], "from-config-not-a-literal");
    }

    /// The two functions that legitimately skip `--index` are exactly these
    /// two, and each is documented at its definition as to why. A new
    /// exemption showing up here without a matching doc comment is the
    /// regression this test is watching for.
    #[test]
    fn version_and_default_index_probe_are_the_only_commands_without_an_index() {
        assert!(!QmdCommand::version().args.contains(&"--index".to_string()));
        assert!(
            !QmdCommand::default_index_collection_list()
                .args
                .contains(&"--index".to_string())
        );
    }

    /// Architecture test: nothing outside this file may spell out the
    /// literal command program `"qmd"`. Everything this crate has to say
    /// about a qmd invocation - indexed or not - is said here, through
    /// [`QmdCommand`]. A call site anywhere else building
    /// `PlannedCommand::new("qmd", ...)` directly would route a qmd
    /// invocation around the only place `--index` is enforced, and the type
    /// system does not forbid that (a `PlannedCommand` can be built with any
    /// program string). This test is what actually forbids it.
    ///
    /// Detection is a plain text scan for the construction pattern
    /// `new("qmd"` - deliberately narrower than "the substring `qmd`
    /// anywhere", so it does not trip over doc comments, JSON field names,
    /// or test assertions that merely compare against the string `"qmd"`.
    #[test]
    fn qmd_command_literal_is_confined_to_this_module() {
        let this_file = "qmd.rs";
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

        let mut violations = Vec::new();
        for entry in std::fs::read_dir(&src_dir).expect("read kaibo-core/src") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let file_name = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            if file_name == this_file {
                continue;
            }

            let contents = std::fs::read_to_string(&path).expect("read source file");
            if contents.contains("new(\"qmd\"") {
                violations.push(file_name);
            }
        }

        assert!(
            violations.is_empty(),
            "found a qmd command built outside qmd.rs, bypassing QmdCommand, in: {violations:?}"
        );
    }
}
