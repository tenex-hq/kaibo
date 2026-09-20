#!/usr/bin/env python3
"""Drive both arms of the H2 experiment.

Issue #33. Arm A is the five standards as prose; arm B is the same five
compiled to a rubric. Everything else is held fixed: same model, same
artifacts, same one-turn budget, same k.

This spends real money. `--dry-run` is free and prints exactly what would be
sent, so the prompts can be reviewed before any arm runs.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).parent
STANDARDS = HERE / "standards" / "standards.md"
RUBRIC = HERE / "arms" / "rubric.json"
UNDER_TEST = HERE / "artifacts" / "under-test"
RUNS = HERE / "runs"

# Arm A must receive the standards' page text and nothing that only exists
# because an experiment is happening. Everything above the horizontal rule in
# standards.md is scaffolding for the reader of this repo.
SPLIT = "\n---\n"


def prose_standards() -> str:
    text = STANDARDS.read_text()
    if SPLIT not in text:
        sys.exit("standards.md has lost its '---' separator; refusing to guess")
    body = text.split(SPLIT, 1)[1].strip()
    # The ids exist for the truth set and the grader. A corpus page carries a
    # title, not an id, so the prose arm is handed the headings without them.
    return "\n".join(
        line.split(" - ", 1)[1] if line.startswith("## t") and " - " in line else line
        for line in body.splitlines()
    )


def arm_a_prompt(artifact_name: str, artifact: str) -> str:
    return f"""Review the Rust test code below against the standards that follow it.

# The standards

{prose_standards()}

# The artifact: {artifact_name}

```rust
{artifact}
```

Review this artifact against these standards. Report what you find."""


def render_rubric(rubric: dict) -> str:
    out = []
    for s in rubric["standards"]:
        j = s["judgment"]
        locators = "\n".join(f"  - {loc}" for loc in j["locators"])
        out.append(
            f"""## {s['id']} - {s['title']}
severity: {s['severity']}

rule:
{s['rule']}

criterion: {j['criterion']}

ask: {j['ask']}

verdict_values: {json.dumps(j['verdict_values'])}

locators:
{locators}

response_schema:
```json
{json.dumps(s['response_schema'], indent=2)}
```"""
        )
    return "\n\n".join(out)


def arm_b_prompt(artifact_name: str, artifact: str, rubric: dict) -> str:
    return f"""You have been handed a conformance contract: {len(rubric['standards'])} standards, each with a judgment item. \
Answer every item. An item you skip is a dropped verdict, not a pass.

# The contract (version {rubric['contract_version']})

{render_rubric(rubric)}

# The artifact: {artifact_name}

```rust
{artifact}
```

Answer every judgment item, in the response schema each one names."""


def claude(prompt: str, model: str, budget: float) -> dict:
    proc = subprocess.run(
        [
            "claude", "-p",
            "--safe-mode",                 # no CLAUDE.md, skills, plugins, hooks, MCP
            "--model", model,
            "--tools", "",                 # no filesystem: the artifact is the prompt
            "--strict-mcp-config",
            "--no-session-persistence",
            "--permission-prompts", "none",
            "--max-budget-usd", str(budget),
            "--output-format", "json",
        ],
        input=prompt,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        return {"harness_error": proc.stderr.strip() or f"exit {proc.returncode}"}
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError:
        return {"harness_error": "unparsable output", "stdout": proc.stdout[:4000]}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="claude-sonnet-5")
    ap.add_argument("--k", type=int, default=3)
    ap.add_argument("--budget", type=float, default=0.50, help="per-call USD ceiling")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    if not RUBRIC.exists():
        sys.exit(f"no rubric at {RUBRIC}")
    artifacts = sorted(UNDER_TEST.glob("*.rs"))
    if not artifacts:
        sys.exit(f"no artifacts in {UNDER_TEST}")
    rubric = json.loads(RUBRIC.read_text())

    prompts = {}
    for path in artifacts:
        body = path.read_text()
        prompts[("A", path.name)] = arm_a_prompt(path.name, body)
        prompts[("B", path.name)] = arm_b_prompt(path.name, body, rubric)

    if args.dry_run:
        for (arm, name), text in sorted(prompts.items()):
            print(f"===== arm {arm} / {name} ===== {len(text)} chars")
            print(text)
            print()
        chars = sum(len(t) for t in prompts.values()) * args.k
        print(f"--- {len(prompts)} prompts x k={args.k}, ~{chars // 4} input tokens total", file=sys.stderr)
        return

    stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    out = RUNS / stamp
    (out / "raw").mkdir(parents=True)
    records = []
    for (arm, name), text in sorted(prompts.items()):
        for rep in range(1, args.k + 1):
            tag = f"{arm}-{name}-r{rep}"
            print(f"running {tag}", file=sys.stderr)
            res = claude(text, args.model, args.budget)
            (out / "raw" / f"{tag}.json").write_text(json.dumps(res, indent=2))
            usage = (res.get("modelUsage") or {}).get(args.model, {})
            records.append({
                "tag": tag, "arm": arm, "artifact": name, "repeat": rep,
                "error": res.get("harness_error"),
                "answer": res.get("result"),
                "input_tokens": usage.get("inputTokens", 0)
                + usage.get("cacheReadInputTokens", 0)
                + usage.get("cacheCreationInputTokens", 0),
                "output_tokens": usage.get("outputTokens", 0),
                "cost_usd": usage.get("costUSD", 0.0),
            })

    (out / "manifest.json").write_text(json.dumps({
        "issue": 33,
        "model": args.model,
        "k": args.k,
        "artifacts": [p.name for p in artifacts],
        "rubric_sha256": hashlib.sha256(RUBRIC.read_bytes()).hexdigest(),
        "standards_sha256": hashlib.sha256(STANDARDS.read_bytes()).hexdigest(),
        "prompt_sha256": {f"{a}/{n}": hashlib.sha256(t.encode()).hexdigest()
                          for (a, n), t in sorted(prompts.items())},
        "records": records,
    }, indent=2))
    print(f"\nwrote {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
