# Eval case design

What we are measuring and why. The file format lands separately; this is the
matrix, and it is the part worth arguing about.

Two failure directions, not one. Everything before 0.4.0 measured only whether
kaibo fires. Over-firing is also a defect - a plugin that queries the corpus on
every bash question burns latency and trust, and would be scored as a success by
a trigger-rate metric alone. Cases 5 and 6 exist to hold that line.

| # | Case | Prompt shape | Pass |
|---|---|---|---|
| 1 | Domain entry, no local evidence | "lets do observability", empty dir | `kaibo:query` is the first tool call |
| 2 | Domain entry, competing local evidence | dev-tooling question, inside a repo that appears to answer it | queries kaibo AND reads local state; neither alone |
| 3 | Research clause | "should we use X or Y for Z", a decision with no obvious answer | queries kaibo before proposing; asks whether a position exists at all |
| 4 | Gap honesty | question in a domain the corpus does not cover | reports no position; does not invent one or silently answer from general knowledge |
| 5 | Skip, pure syntax | "how do I write a bash for loop" | does NOT query kaibo |
| 6 | Skip, repo-local fact | "what does this function return", answerable from the file in front of it | does NOT query kaibo |
| 7 | Corroboration shape | domain entry in a repo with partial compliance | answer is the delta, not a recitation of doctrine and not repo-only |
| 8 | Reverse finding | repo demonstrably ahead of the corpus | names the corpus as behind; flags a contribute candidate |

## Notes on scoring

Cases 1-3 and 5-6 are mechanical: they are answered by the tool-call sequence
alone, so they grade cheaply and deterministically. Cases 4, 7 and 8 are
judgement calls about the answer's content and need a rubric grader.

Run every case with `--ablation with-without`. The no-plugin arm is what turns
"it answered well" into "it answered better than it would have without kaibo",
which is the only number that justifies the plugin's context cost.

Case 8 is the flywheel probe. It fired unprompted in a real session on
2026-08-29 (the agent led with "this project is ahead of the org's written
doctrine and none of it has been written back"), which is the behaviour worth
protecting against regression - it is also the hardest to construct fairly,
since it needs a repo that genuinely outruns the corpus.

## Unblocked: cases run on Caliper (2026-09-17)

The matrix below is authored and running. Not on `claude plugin eval` - that is
still gated - but on [Caliper](https://github.com/edonadei/caliper), which
offers the same primitives under different names: a declared neighbourhood of
skills, `activates:` scored on its own scoreboard apart from answer quality, and
`--ablate` for the with/without arm.

Cases 1, 3, 5, 6 and the three lane-split probes live in
[`activation.eval.yaml`](activation.eval.yaml); cases 4 and 7 in
[`behaviour.eval.yaml`](behaviour.eval.yaml). Case 2 is approximated and case 8
is not authored - see *Known limits* in [README.md](README.md), which is also
where the commands are.

The section below is kept as the record of why this was parked, and of what was
reverse-engineered about the native runner in case it ever ships.

## Historical: blocked on `claude plugin eval` (2026-08-29)

`claude plugin eval` exists in the CLI (v2.1.236) but is gated behind an
early-access flag not enabled here - `eval init --bare` returns "currently in
early access" and writes nothing. Cases cannot be run or verified, so none are
authored yet: writing them against an unrunnable schema would produce files that
look tested and are not.

What is known, reverse-engineered from strings in the binary (error messages and
flags verbatim and reliable; frontmatter key names inferred):

- Layout: `evals/<case-name>/`, each holding `case.yaml` **or** `prompt.md` +
  `graders/*.md`. Overridable via `--eval-dir` or `experimental.evals` in
  plugin.json.
- Grader types: `regex | tool_order | tool_used | file_exists | llm | baseline`.
  Cases 1-3 and 5-6 above map to `tool_used` and `tool_order`, which grade
  deterministically off the trace. Cases 4, 7, 8 need `llm`, judged by haiku with
  majority voting over several votes.
- Scoring: a run scores the weighted fraction of graders passed, a case is the
  mean of its runs (default 3), the suite is the mean of cases. `--threshold`
  defaults to 1.0 and drives the exit code.
- `--ablation with-without` runs a no-plugin baseline arm. A `tool_used: Skill`
  grader is marked as a plugin-fired indicator and excluded from the ablation
  score, so the delta measures answer quality rather than "did it fire".

Unconfirmed and worth checking before authoring: the frontmatter key for the
tool identifier, and the exact spelling of the with-only marker. One run of
`eval init --bare` on a machine with the flag settles both.

**Un-park trigger:** the early-access flag becoming available. Until then the
matrix above is the deliverable, and the headless-session harness used on
2026-08-29 (`claude -p --output-format stream-json`, grep the tool sequence)
remains the only way to run cases 1, 2, 5 and 6.
