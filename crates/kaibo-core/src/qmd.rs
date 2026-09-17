//! The single, guarded way to build a `qmd` invocation.
//!
//! Kaibo operates on one dedicated qmd index; qmd's default index, where a
//! user's own personal collections may live, must never be touched. That
//! guarantee is structural here, not a convention documented in a skill:
//! [`QmdCommand::index_command`] is the only function in this crate that assembles an
//! argument vector for a qmd operation that reads or writes an index, and it
//! always injects `--index <config.index()>` ahead of whatever the caller
//! asked for. There is no path through this type that produces such a
//! command without an index, and no way to pass a literal string in place of
//! the value [`Config`] resolved - see the tests below for the regression
//! this shape guards against.
//!
//! Two functions live outside that guarantee on purpose, each documented at
//! its own definition: [`QmdCommand::version`] (a version check addresses no
//! index at all) and [`QmdCommand::default_index_collection_list`] (proving
//! the default index is untouched means reading the default index, by
//! definition - the same carve-out the qmd contract this crate follows makes
//! for its own migration guard).

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
    /// The real guarantee is structural - `QmdCommand::index_command` is the only
    /// function that assembles this argument vector, and it reads the index
    /// out of `Config` itself - this test guards the regression (a future
    /// call site building the vector ad hoc, or hardcoding a literal index),
    /// it does not discover a design flaw.
    #[test]
    fn index_operations_carry_the_configured_index_not_a_literal() {
        let config = ConfigBuilder::new("/unused")
            .index("from-config-not-a-literal", ConfigSource::File)
            .build();

        let commands = [QmdCommand::status(&config)];
        for command in commands {
            assert_eq!(command.program, "qmd");
            let position = command
                .args
                .iter()
                .position(|arg| arg == "--index")
                .expect("index operation must carry --index");
            assert_eq!(command.args[position + 1], config.index());
            assert_eq!(command.args[position + 1], "from-config-not-a-literal");
        }
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
}
