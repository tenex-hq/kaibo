# Getting Started

From nothing installed to an agent loading your doctrine and opening its first
knowledge PR.

## 1. Install

```console
brew install tenex-hq/tap/kaibo
# or: curl -LsSf https://github.com/tenex-hq/kaibo/releases/latest/download/kaibo-installer.sh | sh

npm install -g @tobilu/qmd@2.8.3
```

kaibo also needs [`gh`](https://cli.github.com/), authenticated with read access to
the knowledge repo, which may be private. qmd is pinned to the version the
[contract](qmd-contract.md) is verified against; `kaibo status` says when the
installed one differs.

## 2. Start a knowledge repo

Skip this if your team already has one.

A knowledge repo is an ordinary GitHub repository with this shape:

```
_index.md        root map: one section per domain
CODEOWNERS       one line per domain folder
testing/
  reference/     how things are, or should be
  how-to/        task walkthroughs and runbooks
  faq/           short answers to real questions
```

1. Create the repository, public or private.
2. Copy [`template/domain/`](../template/domain/) in once per domain, renamed to
   the domain (`testing/` above). Replace the `_example.md` pages with real ones,
   or delete them.
3. Write `_index.md` with one section per domain, in the format
   [the conventions](../template/CONVENTIONS.md#_indexmd---the-root-moc) fix.
   `doctrine` and `domains` read this file, so a domain missing from it is
   invisible to them.
4. Add a `CODEOWNERS` line per domain: `/testing/ @your-handle`.

[`template/CONVENTIONS.md`](../template/CONVENTIONS.md) is the full rulebook:
frontmatter, wikilinks, binding standards, what makes a good page. `kaibo lint`
checks against it, so wire `kaibo lint` into the knowledge repo's CI.

## 3. Configure

kaibo never hardcodes a knowledge repo. Point it at yours with the `KAIBO_REPO`
environment variable, or the `repo` key in `~/.kaibo/config.toml`:

```toml
repo = "your-org/knowledge"
```

There is no compiled-in default: a verb that needs a repo and has none says so and
names the fix. The precedence is environment, then config file, then default, and
an empty value counts as unset at every layer.

The same file can reparameterise `kaibo lint`'s compiled rules: which frontmatter
keys are required, which `status` values are accepted, how a folder maps to a
`type`, the tag pattern, and which rules are off entirely. None of it can come
from the knowledge repo itself; the module docs on
[`lint.rs`](../crates/kaibo-core/src/lint.rs) say why.

```toml
[lint]
disabled_rules = []

[lint.frontmatter_contract]
required_keys = ["title", "tags", "status", "updated"]
allowed_status = ["draft", "current", "deprecated"]

[lint.frontmatter_contract.type_folder_overrides]
howto = "how-to"

[lint.tags_kebab_case]
pattern = "[a-z0-9]+(-[a-z0-9]+)*"
```

## 4. Bootstrap

```console
kaibo install
kaibo sync
```

`install` writes the skills the binary carries to `~/.claude/skills/kaibo/`, so
Claude Code picks them up in the next session. They tell the agent to load a
domain's doctrine when work enters that domain, and to research before it proposes
or decides. Skills and binary ship as one artefact, so upgrading one upgrades both.

`sync` clones the knowledge repo to `~/.kaibo/knowledge`, registers it in kaibo's
own qmd index, and indexes and embeds it. It is idempotent; the same command keeps
you fresh later. Any qmd collections you use personally are never touched.

## 5. Read

```console
kaibo domains
kaibo doctrine testing
kaibo query "should integration tests hit the network"
```

`domains` lists what the corpus covers. `doctrine` loads one domain's summary plus
its `current` reference pages in a single call: a load, not a question. A name
that is not a domain exits `2` with the real names to choose from, because a wrong
name is not a knowledge gap.

`query` returns ranked passages, each cited as `domain/type/page.md` and fenced as
untrusted content. It never writes an answer of its own; the agent does that from
the evidence. Draft pages are withheld by default, and the output says when one
was and names the `--include-drafts` command that surfaces it.

When nothing matches, `query` exits `3` and lists the known domains. That is a
gap: the organisation has no written position yet.

## 6. Contribute

```console
kaibo contribute plan "integration tests never reach the network; record and replay at the edge"
```

`plan` is read-only. It surfaces placement candidates (which domain, which existing
page to append to, or a new page) and leaves the choice to the caller. Once the
placement is resolved:

```console
kaibo contribute apply \
  --type reference \
  --domain testing \
  --title "Integration Tests Stay Off the Network" \
  --body "..." \
  --tag hermetic-tests
```

`apply` lints the page, and checks its references when `reflock` is on `PATH`,
before anything touches the clone. It refuses to create over a page that already
exists, then branches, commits, pushes
(directly, or through a fork when you lack write access) and opens the PR. It stops
there: CI and review happen on the PR. New pages start as `status: draft` until a
reviewer promotes them.

Pass `--append <path>` to extend an existing page instead, and `--binding` with
`--severity` and `--action` to file a binding standard.

## Staying fresh

`kaibo query` and `kaibo doctrine` sync on their own when the clone is missing,
stale, or its index is gone. Run `kaibo sync` by hand after a knowledge PR merges
if you want it right away, and `kaibo status` when something looks off: it reports
what is wrong and the one command that fixes it.

---

## Appendix: what `kaibo sync` does underneath

`kaibo sync --explain` prints the exact commands for your configuration. In
outline:

```bash
# clone once
gh repo clone <your-configured-repo> ~/.kaibo/knowledge

# refresh: main branch, hooks disabled
git -C ~/.kaibo/knowledge checkout main
git -c core.hooksPath=/dev/null -C ~/.kaibo/knowledge pull

# one collection in a dedicated index; the mask is typed folders inside domain folders
qmd collection add ~/.kaibo/knowledge --index kaibo --name knowledge --mask "*/{reference,how-to,faq}/**/*.md"

# reindex and refresh embeddings, scoped by the index
qmd update --index kaibo
qmd embed --index kaibo
```

[`qmd-contract.md`](qmd-contract.md) has the full command contract.
