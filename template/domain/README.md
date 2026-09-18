# &lt;domain&gt; - domain folder template

> Skeleton for a new **domain folder** in the knowledge monorepo. Copy it into the repo root as `<domain>/`, write real pages, delete the `_example.md` files and this README.

## Adding a domain = one ordinary PR

No registration step, no new repo:

1. Copy this folder into the knowledge monorepo as `<domain>/`.
2. Add a `## <domain>` section to the repo's `_index.md` (owner, domain, topics, summary - describe what actually lives there, not aspiration).
3. Add a `CODEOWNERS` line: `/<domain>/ @your-github-handle`.
4. Open the PR. After merge, run **`/kaibo:sync`** - the collection mask picks up `<domain>/{reference,how-to,faq}/` automatically; no qmd commands needed.

(`/kaibo:contribute` does all of this for you when it files knowledge into a domain that doesn't exist yet.)

## Layout

| Folder | Holds |
|--------|-------|
| `reference/` | Guidelines, commandments, baselines - how things are / should be |
| `how-to/` | How-do-I answers and runbooks - task-oriented steps |
| `faq/` | Answered questions (Q&A pairs) |

Every page carries frontmatter (`type`, `title`, `tags` kebab-case, `status`, `updated`) and may `[[wikilink]]` to any page in the repo - keep page basenames unique repo-wide. See the [Kaibo conventions](../CONVENTIONS.md).
