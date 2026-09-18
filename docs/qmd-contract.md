# QMD Contract

The exact [QMD](https://github.com/tobi/qmd) commands kaibo depends on.
Verified against **qmd 2.8.3**. If QMD's interface changes, update this file
together with the code, and re-verify the load-bearing behaviors with:

```bash
KAIBO_QMD_CONTRACT=1 cargo test -p kaibo-core qmd_contract_check -- --nocapture
```

(see [`crates/kaibo-core/src/status/qmd_contract_check.rs`](../crates/kaibo-core/src/status/qmd_contract_check.rs) - opt-in and non-hermetic, so it never runs in CI by default).

## THE contract: `--index kaibo` on every kaibo call

Kaibo uses a **dedicated QMD index** - a separate database at
`~/.cache/qmd/kaibo.sqlite`, structurally isolated from the default index
(`~/.cache/qmd/index.sqlite`) where the user's personal collections live.
Every kaibo qmd call - `query`, `get`, `collection *`, `update`, `embed`,
`status` - carries `--index kaibo`. Isolation is structural, not a flag we
hope scopes: an operation on the `kaibo` index cannot see, pull, or re-embed
anything in the default index.

**Where qmd keeps state** (verified 2.8.3): collection *definitions* (name →
path + mask) live in `~/.config/qmd/<index>.yml`; the sqlite at
`~/.cache/qmd/<index>.sqlite` is derived index data. Always add/remove
collections via `qmd collection add/remove` - deleting a sqlite file alone
leaves orphaned definitions in the yml. Collection names are unique **per
index**, so kaibo's `knowledge` cannot collide with a user's same-named
personal collection in the default index.

## One collection

There is exactly one collection: **`knowledge`**, pointing at the configured
knowledge repo's local clone.

```
$ qmd status --index kaibo
QMD Status

Index: ~/.cache/qmd/kaibo.sqlite
Size:  1.4 MB

Documents
  Total:    21 files indexed
  Vectors:  38 embedded
  Pending:  0 need embedding
  Updated:  2h ago

Collections
  knowledge (qmd://knowledge/)
    Pattern:  */{reference,how-to,faq}/**/*.md
    Files:    21 (updated 2h ago)
```

`kaibo sync` creates it as:

```bash
qmd collection add ~/.kaibo/knowledge --index kaibo --name knowledge --mask "*/{reference,how-to,faq}/**/*.md"
```

**Mask semantics** (verified against a fixture): matches
`<domain>/{reference,how-to,faq}/**/*.md` - i.e. typed folders nested one
level down. Root files (`README.md`, `_index.md`, `CODEOWNERS`) and stray
domain-level `.md` files are **not** indexed. Extend the mask if a new
content-type folder (e.g. `tutorial/`) is ever added.

## Updating / refreshing - correct semantics

```bash
qmd update --index kaibo        # re-index the kaibo index's collections from disk
qmd embed --index kaibo         # (re)generate pending vector embeddings
```

- **`qmd update` scopes per-index, and only per-index.** `-c/--collection` is a
  *search* option; passing it to `update` is silently ignored.
- `qmd update --pull` also git-pulls every collection directory in the index.
  Kaibo doesn't use it - `kaibo sync` pulls explicitly with hooks disabled,
  then runs a plain `qmd update --index kaibo`.
- Fresh content isn't fully retrievable until `qmd embed` has run - `update`
  alone leaves embeddings pending and degrades hybrid retrieval.

## Querying - the shape kaibo parses

Scope with `-c` (**legitimate here** - it's a search-time filter) and ask for
JSON. Progress output goes to **stderr**, so redirect it when capturing:

```bash
qmd query "<natural question>" -c knowledge --index kaibo --json 2>/dev/null
```

- `qmd query` - hybrid: auto query-expansion + BM25 + vector + LLM rerank.
  **Default choice** for `kaibo query`.
- `qmd search` - BM25 keywords only (fast, no LLM). Good for exact identifiers.
- `qmd vsearch` - vector similarity only.

### `--json` output shape

An array of hits, best first:

```json
[
  {
    "docid": "#e14e79",
    "score": 0.88,
    "file": "qmd://knowledge/kaibo/how-to/add-a-knowledge-domain.md",
    "title": "Add a Knowledge Domain",
    "snippet": "@@ -35,4 @@ (34 before, 17 after)\n..."
  }
]
```

- `file` is `qmd://knowledge/<domain>/<type>/<page>.md`. Strip the
  `qmd://knowledge/` prefix to get the citation path -
  `<domain>/<type>/<page>.md` - with the domain visible as the first segment.
- `snippet` is a diff-style excerpt with line context - useful for citing the
  exact region.
- Fetch fuller context with
  `qmd get "qmd://knowledge/<domain>/<type>/<page>.md" --index kaibo -l 40`.

## Gotchas

- Always `2>/dev/null` when capturing `--json` - the progress spinner is on
  stderr but noisy.
- **Query grammar:** a query starting with `expand:`, `lex:`, `vec:`, `hyde:`,
  or `intent:` is parsed as a structured query document - strip such prefixes
  from user questions before passing them through. Double quotes inside a
  question break shell quoting - single-quote it.
- `qmd query` invokes a local LLM for expansion/rerank (a few seconds);
  `qmd search` doesn't. Use `search` when latency matters and the query is
  keyword-exact.
- A fresh clone isn't retrievable until `kaibo sync` has registered, indexed,
  and embedded it. Don't run ad-hoc `qmd collection add`/`qmd update` for
  kaibo content - that's `sync`'s job.

## When qmd upgrades

Run the contract check (see above). It verifies, against a throwaway scratch
index (read-only for your default index): (a) `--index` isolation, (b) the
domain-nested mask, (c) that `qmd update --index <x>` leaves the default index
untouched.
