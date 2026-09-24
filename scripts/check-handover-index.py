#!/usr/bin/env python3
"""Check HANDOVER.md's contents index against the headings it claims to map.

    scripts/check-handover-index.py [path/to/HANDOVER.md]

Exits 0 when every link resolves, every section is listed, the order matches
the document and the count in the <summary> is right; exits 1 naming what to
fix. Run it after adding a section — the index is hand-maintained, and a map
that has quietly stopped matching the territory is worse than no map.

Why it asks GitHub instead of slugging the headings here: the obvious
implementation is a regex of one's own, and it is wrong in a way that looks
right. github-slugger strips `×`, `§`, `→` and the rest of the Latin-1 and
arrow blocks; a plausible hand-rolled version keeps them, and on 2026-09-23 it
reported four perfectly good links broken with complete confidence. The
renderer is the only thing whose answer counts, so this posts the headings to
it and compares against the ids it actually emits.

Needs `gh` on PATH and authenticated (the same dependency the release flow
already has) and a network. Only the heading lines are sent, never the body:
anchors depend on heading text and on the order duplicates appear in, both of
which survive sending the headings alone.
"""
import json, re, subprocess, sys
from pathlib import Path

HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
FENCE = re.compile(r"^\s*(```|~~~)")
ENTRY = re.compile(r"^\s*- \[(.+?)\]\(#(.+?)\)\s*$")
COUNT = re.compile(r"<summary>.*?(\d+) sections", re.S)


def parse(text):
    """Headings (outside code fences and outside the index) and index entries.

    The index block is skipped when collecting headings for the same reason
    fenced blocks are: its lines are links, not sections, and counting them
    would make the file appear to document itself.
    """
    heads, entries, fenced, in_index = [], [], False, False
    for n, line in enumerate(text.split("\n"), 1):
        if line.startswith("<details>"):
            in_index = True
        elif line.startswith("</details>"):
            in_index = False
            continue
        if FENCE.match(line):
            fenced = not fenced
            continue
        if fenced:
            continue
        if in_index:
            m = ENTRY.match(line)
            if m:
                entries.append((n, m.group(1), m.group(2)))
            continue
        m = HEADING.match(line)
        if m:
            heads.append((n, len(m.group(1)), m.group(2)))
    return heads, entries


def github_anchors(heads):
    """The ids GitHub itself generates for these headings, in order."""
    payload = {"text": "\n\n".join(f"{'#' * lvl} {t}" for _, lvl, t in heads) + "\n",
               "mode": "markdown"}
    try:
        out = subprocess.run(
            ["gh", "api", "--method", "POST", "/markdown", "--input", "-"],
            input=json.dumps(payload), capture_output=True, text=True, check=True).stdout
    except FileNotFoundError:
        sys.exit("gh not found on PATH — this check needs it to reach the renderer.")
    except subprocess.CalledProcessError as e:
        sys.exit(f"gh could not reach the markdown API:\n{e.stderr.strip()}")
    return re.findall(r'id="user-content-([^"]+)"', out)


def main():
    default = Path(__file__).resolve().parent.parent / "HANDOVER.md"
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else default
    text = path.read_text(encoding="utf-8")
    heads, entries = parse(text)
    if not entries:
        sys.exit(f"{path}: no contents index found (no `- [text](#anchor)` inside <details>).")

    ids = github_anchors(heads)
    if len(ids) != len(heads):
        sys.exit(f"renderer returned {len(ids)} anchors for {len(heads)} headings — cannot compare.")
    anchor_of = {t: a for (_, _, t), a in zip(heads, ids)}
    rendered = set(ids)
    problems = []

    for n, label, anchor in entries:
        if anchor not in rendered:
            want = anchor_of.get(label)
            fix = f" — for this heading GitHub generates #{want}" if want else ""
            problems.append(f"line {n}: [{label}](#{anchor}) resolves to nothing{fix}")

    # The H1 is the document title; it is deliberately not indexed.
    listed = {a for _, _, a in entries}
    for (n, lvl, t), a in zip(heads, ids):
        if lvl > 1 and a not in listed:
            problems.append(f"line {n}: section {t!r} is not in the index (add #{a})")

    doc_order = [t for _, lvl, t in heads if lvl > 1]
    if doc_order != [label for _, label, _ in entries]:
        for i, (a, b) in enumerate(zip(doc_order, [l for _, l, _ in entries])):
            if a != b:
                problems.append(f"index order diverges at entry {i + 1}: expected {a!r}, found {b!r}")
                break

    m = COUNT.search(text)
    if m and int(m.group(1)) != len(entries):
        problems.append(f"<summary> says {m.group(1)} sections; the index lists {len(entries)}")

    if problems:
        print(f"{path}: {len(problems)} problem(s)")
        for p in problems:
            print(f"  {p}")
        return 1
    print(f"{path}: {len(entries)} entries, all resolving, in document order.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
