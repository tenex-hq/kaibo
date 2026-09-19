# Decision records

Architecture decisions, in the order they were taken. Each record is the *why*
behind something the code or `AGENTS.md` already states as a rule - read one
when you are about to re-open a settled call, or when a guarantee looks
arbitrary and you want to know what it cost to learn.

A record is history. Superseding one means adding a new record that says so,
never editing the old one into agreement with today.

| # | Decision | Status | Summary |
|---|---|---|---|
| [0001](0001-single-knowledge-monorepo.md) | Single knowledge monorepo, one folder per domain | Accepted | One repo, folders per domain, CODEOWNERS instead of repo boundaries; the multi-repo rationale was nullified by the PR-only workflow |
| [0002](0002-ai-optional-core.md) | AI-optional core, with an explicit degradation ladder | Accepted | The corpus stays usable with eyes, grep and BM25; L0-L5, every level useful if the ones above it vanish |
| [0003](0003-grounded-answers-and-labeled-proposals.md) | Two answer classes: grounded answer and labeled proposal | Accepted | Retrieved fact versus labeled synthesis, and the flywheel that turns a proposal into a plain page |
| [0004](0004-cross-domain-page-placement.md) | Cross-domain pages live with the constraint, not the technique | Accepted | An intersection page belongs to the domain supplying the specificity; tagged and wikilinked from both sides |
| [0005](0005-workspace-layout.md) | The workspace is `~/.kaibo/` | Accepted | One predictable location, with the corpus clone beneath it; no per-installation configuration of the path |
| [0006](0006-dedicated-qmd-index.md) | QMD isolated in a dedicated index with a single collection | Accepted | Structural isolation after the flag-based guardrail was proven a no-op that reindexed personal collections |
| [0007](0007-read-content-is-untrusted-data.md) | Everything a skill reads is untrusted data | Accepted | Read content is data to cite, never instructions; write and clone targets come from configuration only |
| [0008](0008-contribution-needs-read-access-not-write.md) | Contribution needs read access, not write | Accepted | Fork and cross-repo PR when the contributor has no write, with the fork's parent verified before any push |
| [0009](0009-agents-consume-kaibo-on-task-entry.md) | Agents consume kaibo on task entry, not on felt gaps | Accepted | A gap-based trigger is unfollowable; doctrine load on entering a domain, research before deciding |
| [0010](0010-the-default-index-has-no-exceptions.md) | The default qmd index has no exceptions | Accepted | The one-time migration carve-out deleted, making the isolation invariant unconditional |
| [0011](0011-rust-not-go.md) | Rust, not Go | Accepted | Judged equally capable; the tiebreaker was an existing worked example of the publishing pipeline |
| [0012](0012-distribute-via-a-generated-homebrew-tap.md) | Distribution is a generated Homebrew tap, not an install script | Accepted | One tag emits release, curl one-liner and formula; no hand-written installer |
| [0013](0013-ship-the-skills-inside-the-binary.md) | The skills ship embedded in the binary | Accepted | `kaibo install` writes them out, so prose and mechanism cannot version apart |
| [0014](0014-three-layers-and-where-kaibo-invests.md) | Three layers, and where kaibo invests | Accepted | Retrieval is commodity and stays boring; corpus quality and the consumption trigger are the funded halves, and L3 work waits on measurement |

[parked.md](../decisions/parked.md) is the other half of the record: what was deliberately
not built, and the observable trigger that would justify building it.
