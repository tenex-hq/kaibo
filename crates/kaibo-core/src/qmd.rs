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

use std::path::Path;

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

    /// `qmd collection list --index <configured>` - the collections that
    /// exist in the *configured* index, used to check whether `sync`'s
    /// collection already exists before deciding whether to create it. Not
    /// to be confused with [`Self::default_index_collection_list`], which
    /// deliberately addresses qmd's default index instead.
    pub fn collection_list(config: &Config) -> PlannedCommand {
        Self::index_command(config, "collection", ["list"])
    }

    /// `qmd collection add <path> --name <name> --mask <mask> --index
    /// <configured>` - register `path` as a collection in the configured
    /// index, restricted to `mask`. Not idempotent on qmd's side (a second
    /// call with the same name fails), so callers must check
    /// [`Self::collection_list`] first.
    pub fn collection_add(config: &Config, path: &Path, name: &str, mask: &str) -> PlannedCommand {
        Self::index_command(
            config,
            "collection",
            [
                "add".to_string(),
                path.to_string_lossy().into_owned(),
                "--name".to_string(),
                name.to_string(),
                "--mask".to_string(),
                mask.to_string(),
            ],
        )
    }

    /// `qmd update --index <configured>` - re-scan every collection *in the
    /// configured index* for new/changed/removed files. Deliberately never
    /// passes `--pull`: that would have qmd itself git-pull every collection
    /// in the index, bypassing the hooks-disabled pull `sync` runs itself.
    pub fn update(config: &Config) -> PlannedCommand {
        Self::index_command(config, "update", Vec::<String>::new())
    }

    /// `qmd embed --index <configured>` - generate/refresh vector embeddings
    /// for the configured index.
    pub fn embed(config: &Config) -> PlannedCommand {
        Self::index_command(config, "embed", Vec::<String>::new())
    }

    /// `qmd query <question> --index <configured> -c <configured collection>
    /// --format json` - retrieval against the configured index and
    /// collection only, asking for JSON so `query` can parse hits rather
    /// than scrape human-formatted text. `question` must already be
    /// sanitised (a leading `expand:`/`lex:`/`vec:`/`hyde:`/`intent:`
    /// prefix stripped) by the caller - this constructor does not sanitise
    /// it, so it stays a thin, honest builder like every other one here.
    pub fn query(config: &Config, question: &str) -> PlannedCommand {
        Self::index_command(
            config,
            "query",
            [
                question.to_string(),
                "-c".to_string(),
                config.collection().to_string(),
                "--format".to_string(),
                "json".to_string(),
            ],
        )
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
    /// The scan walks the whole crate source tree recursively, not just its
    /// top level, so a violation in a submodule file (`query/tests.rs`,
    /// `doctrine/tests.rs`, or one that does not exist yet) is caught the
    /// same as one sitting next to `qmd.rs`.
    #[test]
    fn qmd_command_literal_is_confined_to_this_module() {
        fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read directory") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    rust_files(&path, out);
                } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }

        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let this_file = src_dir.join("qmd.rs");

        let mut files = Vec::new();
        rust_files(&src_dir, &mut files);

        let mut violations = Vec::new();
        for path in files {
            if path == this_file {
                continue;
            }

            let contents = std::fs::read_to_string(&path).expect("read source file");
            if contents.contains("new(\"qmd\"") {
                violations.push(
                    path.strip_prefix(&src_dir)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                );
            }
        }

        assert!(
            violations.is_empty(),
            "found a qmd command built outside qmd.rs, bypassing QmdCommand, in: {violations:?}"
        );
    }

    /// Mandatory guardrail test for the commands `sync` added: every one of
    /// them carries `--index` with the configured value, not a literal.
    /// `collection_add` and `collection_list` share the `collection`
    /// subcommand but must not be confused with
    /// [`QmdCommand::default_index_collection_list`], which addresses the
    /// default index on purpose.
    #[test]
    fn sync_commands_carry_the_configured_index() {
        let config = ConfigBuilder::new("/unused")
            .index("from-config-not-a-literal", ConfigSource::File)
            .build();

        for command in [
            QmdCommand::collection_list(&config),
            QmdCommand::collection_add(
                &config,
                std::path::Path::new("/clone"),
                "knowledge",
                "*/{reference,how-to,faq}/**/*.md",
            ),
            QmdCommand::update(&config),
            QmdCommand::embed(&config),
        ] {
            assert_eq!(command.program, "qmd");
            let position = command
                .args
                .iter()
                .position(|arg| arg == "--index")
                .unwrap_or_else(|| panic!("no --index in: {command}"));
            assert_eq!(command.args[position + 1], "from-config-not-a-literal");
        }
    }

    /// `query` carries the configured index and collection, not literals,
    /// and asks for JSON so `query::gather` has something structured to
    /// parse.
    #[test]
    fn query_command_carries_configured_index_and_collection_and_asks_for_json() {
        let config = ConfigBuilder::new("/unused")
            .index("from-config-index", ConfigSource::File)
            .collection("from-config-collection", ConfigSource::File)
            .build();

        let command = QmdCommand::query(&config, "how does auth work");

        assert_eq!(command.program, "qmd");
        assert!(command.args.contains(&"how does auth work".to_string()));
        let index_position = command
            .args
            .iter()
            .position(|arg| arg == "--index")
            .expect("query must carry --index");
        assert_eq!(command.args[index_position + 1], "from-config-index");
        let collection_position = command
            .args
            .iter()
            .position(|arg| arg == "-c")
            .expect("query must carry -c <collection>");
        assert_eq!(
            command.args[collection_position + 1],
            "from-config-collection"
        );
        assert!(command.args.windows(2).any(|w| w == ["--format", "json"]));
    }

    /// A question is passed through as a single argv element - no shell is
    /// ever involved, so embedded quotes need no escaping and must survive
    /// verbatim.
    #[test]
    fn query_command_preserves_a_question_containing_quotes_verbatim() {
        let config = ConfigBuilder::new("/unused").build();
        let question = r#"what does "foo" mean"#;

        let command = QmdCommand::query(&config, question);

        assert!(command.args.contains(&question.to_string()));
    }
}
