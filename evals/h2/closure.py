#!/usr/bin/env python3
"""Measure closure: how many standards each attempt actually answered.

Issue #75. H2 measured which defects an arm found. This measures something
else: which of the five standards an arm *addressed at all*, whether it found a
problem or not.

The distinction is the whole claim. Arm B is obliged to return a verdict per
standard, so an item it drops is countable. Arm A simply omits what it did not
consider, and that omission is indistinguishable from a clean bill.

Both arms go through the identical extraction, because arm B's schema makes
closure look guaranteed and the interesting question is whether it holds when
the caller is under pressure. Asserting it from the schema would be asserting
the schema.
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys

RULES = {
    "t1": "An expected value never comes from the code under test",
    "t2": "Every test must be able to fail for the reason its name gives",
    "t3": "A test describes a situation, not a function call",
    "t4": "Tests are hermetic by default",
    "t5": "Guardrail tests sweep, they do not enumerate",
}

SCHEMA = {
    "type": "object",
    "properties": {
        "coverage": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "rule": {"enum": list(RULES)},
                    "state": {"enum": ["flagged", "cleared", "absent"]},
                },
                "required": ["rule", "state"],
            },
        }
    },
    "required": ["coverage"],
}


def main() -> None:
    run = pathlib.Path(sys.argv[1])
    model = sys.argv[2] if len(sys.argv) > 2 else "claude-sonnet-5"
    manifest = json.loads((run / "manifest.json").read_text())
    rows = []
    for rec in manifest["records"]:
        if not rec.get("answer"):
            continue
        print(f"closure {rec['tag']}", file=sys.stderr)
        listing = "\n".join(f"- {k}: {v}" for k, v in RULES.items())
        proc = subprocess.run(
            ["claude", "-p", "--safe-mode", "--model", model, "--tools", "",
             "--strict-mcp-config", "--no-session-persistence",
             "--permission-prompts", "none", "--output-format", "json",
             "--json-schema", json.dumps(SCHEMA)],
            input=f"""Below is one reviewer's report on a Rust test file. For each of the five \
standards listed, say whether the report addressed it.

- `flagged` - the report asserts this standard is violated somewhere in the file.
- `cleared` - the report explicitly says this standard is met, or explicitly says \
it does not apply. The reviewer considered it and said so.
- `absent` - the report neither flags nor clears it. Silence counts as absent, \
however thorough the rest of the report is. Do not credit a standard because the \
report discusses a nearby topic; it must be recognisably about this standard.

Judge only what the report says. Do not read the standards yourself and do not \
decide whether the reviewer was right.

# The standards

{listing}

# The report

{rec['answer']}""",
            capture_output=True, text=True, check=False)
        if proc.returncode != 0:
            sys.exit(f"claude failed: {proc.stderr.strip()[:300]}")
        cov = json.loads(json.loads(proc.stdout)["result"])["coverage"]
        seen = {c["rule"]: c["state"] for c in cov}
        rows.append({"arm": rec["arm"], "artifact": rec["artifact"],
                     "repeat": rec["repeat"],
                     **{r: seen.get(r, "absent") for r in RULES}})
    (run / "closure.json").write_text(json.dumps(rows, indent=2))

    print(f"\n# closure, {manifest['model']}\n")
    print("| arm | addressed | flagged | cleared | silently absent |")
    print("|---|---|---|---|---|")
    for arm in ("A", "B"):
        mine = [r for r in rows if r["arm"] == arm]
        cells = [r[k] for r in mine for k in RULES]
        n = len(cells)
        print(f"| {arm} | {(n - cells.count('absent')) / n:.2f} "
              f"| {cells.count('flagged')} | {cells.count('cleared')} "
              f"| {cells.count('absent')} |")
    print("\n| arm | " + " | ".join(RULES) + " |")
    print("|---" * 6 + "|")
    for arm in ("A", "B"):
        mine = [r for r in rows if r["arm"] == arm]
        print(f"| {arm} absent | " + " | ".join(
            str(sum(1 for r in mine if r[k] == "absent")) for k in RULES) + " |")


if __name__ == "__main__":
    main()
