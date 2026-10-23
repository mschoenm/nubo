#!/usr/bin/env python3
"""Checks for a full review (.claude/skills/full-review/SKILL.md).

  review.py areas                 every tracked file is in exactly one area
  review.py ledger <work folder>  every finding has exactly one outcome

The work folder holds findings/<AREA>.md (headings `### <AREA>-<n>: <title>`)
and ledger.md (one line per finding: `- <ID> <OUTCOME>: <what was done>`).
"""
import pathlib
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
OUTCOMES = ("FIX-docs", "FIX-style", "FIX-idioms", "FIX-bugs", "FIX-tooling", "ISSUE", "NOTE", "MOOT", "WRONG")
BINARY = re.compile(r"\.(png|jpe?g|ico|icns|state|gbc?|sav|rtc|zip|bin|ttf|otf|woff2?)$", re.IGNORECASE)


def areas() -> int:
    rules = []
    for line in (HERE / "areas.txt").read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            area, prefix = line.split()
            rules.append((area, prefix))
    root = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
    files = subprocess.run(["git", "ls-files"], cwd=root, capture_output=True, text=True, check=True).stdout.splitlines()
    counts: dict[str, int] = {}
    unmatched = []
    for path in files:
        if BINARY.search(path):
            continue
        area = next((area for area, prefix in rules if path == prefix or path.startswith(prefix)), None)
        if area is None:
            unmatched.append(path)
        else:
            counts[area] = counts.get(area, 0) + 1
    for area, count in sorted(counts.items()):
        print(f"{area:14} {count:5} files")
    for path in unmatched:
        print(f"in no area: {path}")
    return 1 if unmatched else 0


def ledger(folder: str) -> int:
    work = pathlib.Path(folder)
    ids = []
    for findings in sorted((work / "findings").glob("*.md")):
        ids += re.findall(r"^### ([A-Z][A-Z-]*-\d+)\b", findings.read_text(), re.MULTILINE)
    outcomes: dict[str, list[str]] = {}
    for line in (work / "ledger.md").read_text().splitlines():
        match = re.match(r"^- ([A-Z][A-Z-]*-\d+) ([A-Za-z-]+):", line)
        if match:
            outcomes.setdefault(match.group(1), []).append(match.group(2))
    problems = []
    for finding in sorted({finding for finding in ids if ids.count(finding) > 1}):
        problems.append(f"{finding}: {ids.count(finding)} findings with this ID")
    for finding in dict.fromkeys(ids):
        found = outcomes.get(finding, [])
        if len(found) != 1:
            problems.append(f"{finding}: {len(found)} outcomes {found}")
        elif found[0] not in OUTCOMES:
            problems.append(f"{finding}: unknown outcome {found[0]}")
    for finding in outcomes.keys() - set(ids):
        problems.append(f"{finding}: in the ledger but in no findings file")
    print(f"{len(ids)} findings, {len(problems)} problems")
    for problem in problems:
        print(problem)
    return 1 if problems else 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["areas"]:
        sys.exit(areas())
    if sys.argv[1:2] == ["ledger"] and len(sys.argv) == 3:
        sys.exit(ledger(sys.argv[2]))
    sys.exit(__doc__)
