# Getting Started

From zero to `kaibo query` and `kaibo contribute` working.

## Prerequisites

- [`gh`](https://cli.github.com/) authenticated with access to your configured knowledge repo (it may be private).
- [`qmd`](https://github.com/tobi/qmd) installed and pinned to the version this contract is verified against - see [`qmd-contract.md`](qmd-contract.md).
- `kaibo` itself. See the [README](../README.md#install) for the current install path.

## 1. Configure

Kaibo never hardcodes a knowledge repo. Point it at yours with the `KAIBO_REPO`
environment variable, or the `repo` key in `~/.kaibo/config.toml`. There is no
compiled-in default - a verb that needs a repo and doesn't have one reports
that as its own error. See `Config::resolve()` in
[`crates/kaibo-core/src/config.rs`](../crates/kaibo-core/src/config.rs) for the
full precedence: environment, then config file, then default.

The same file can carry a `[lint]` table to reparameterise `kaibo lint`'s
compiled rules - which frontmatter keys are required, which `status` values
are accepted, how a folder maps to an expected `type`, and the tag pattern -
plus `lint.disabled_rules` to turn a rule off entirely. None of this can come
from the knowledge repo itself: see the module docs on
[`crates/kaibo-core/src/lint.rs`](../crates/kaibo-core/src/lint.rs) for why,
and for the full shape of the table:

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

## 2. Bootstrap

```
kaibo sync
```

Clones the configured knowledge repo (to `~/.kaibo/knowledge` by default),
registers it as a QMD collection in kaibo's own dedicated index, then indexes
and embeds it. Idempotent - the same command keeps you fresh later.

- Kaibo's qmd state is fully isolated in its own index - any QMD collections
  you use personally are never touched.

## 3. Query

```
kaibo query "when should I do sampling in otel"
```

Semantic-searches the corpus and returns a **cited** answer - citations are
`domain/type/page.md`, so the domain is always visible. Pass
`--include-drafts` to see draft pages, each labelled as such. `query` never
synthesises an answer or calls a model of its own; the calling agent does that
with the returned evidence.

## 4. Load a domain's doctrine

```
kaibo doctrine <domain>
kaibo domains
```

`doctrine` loads a domain's own summary plus its current reference pages in
one call - a load, not a question. `domains` lists the available domain
names.

## 5. Contribute

```
kaibo contribute plan "a knowledge page should keep its basename unique - wikilinks resolve repo-wide"
```

`plan` is read-only: it classifies the content and surfaces placement
candidates without writing anything. Once a placement is resolved:

```
kaibo contribute apply \
  --type how-to \
  --domain kaibo \
  --title "Example page" \
  --body "..." \
  --tag example
```

`apply` writes the page, lints it, branches, commits, pushes (directly or via
a verified fork), opens a PR against the configured repo, and watches CI.

## Keeping fresh

Run `kaibo sync` whenever knowledge might be stale - after a PR merges, or
before an important query. `kaibo query` and `kaibo doctrine` also self-heal
by syncing automatically when the local corpus is missing, stale, or its qmd
collection is gone.

---

## Appendix: what `kaibo sync` does under the hood

If you prefer to run it by hand, or want to understand the moving parts:

```bash
# clone (creates ~/.kaibo on the way; only needed once)
gh repo clone <your-configured-repo> ~/.kaibo/knowledge

# refresh: main branch, hooks disabled on pull
git -C ~/.kaibo/knowledge checkout main
git -c core.hooksPath=/dev/null -C ~/.kaibo/knowledge pull

# one collection in a dedicated index; mask = typed folders inside domain folders
qmd collection add ~/.kaibo/knowledge --index kaibo --name knowledge --mask "*/{reference,how-to,faq}/**/*.md"

# reindex + refresh embeddings - scoped structurally by the index
qmd update --index kaibo
qmd embed --index kaibo

# ask something
qmd query "how do I add a domain" -c knowledge --index kaibo --json --explain 2>/dev/null
```

See [`qmd-contract.md`](qmd-contract.md) for the exact QMD command contract.
