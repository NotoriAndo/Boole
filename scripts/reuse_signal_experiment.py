#!/usr/bin/env python3
"""T0 -- reuse-signal offline experiment.

See `local-docs/todo/thesis-realization-roadmap.md`, section
"## T0 -- reuse-signal offline experiment", for the full spec this
implements. Short version: measure whether "value proportional to
downstream reuse" (thesis Sec 2, PageRank-of-lemmas) is a real, non-trivial
signal on a corpus of Lean source files, *before* any conjecture-economy
code is written.

Method (v1, deliberately lightweight -- see module docstring caveats below):
  1. Enumerate corpus declarations (`theorem`/`lemma`/`def` names) and their
     source body (the text from the declaration's own signature up to the
     start of the next declaration in the same file, or end of file).
  2. For each declaration's body, detect word-boundary occurrences of every
     OTHER corpus declaration's name. That declaration -> name edge is one
     entry in the premise DAG (a declaration referencing itself does not
     count; repeated textual references to the same premise still count as
     a single DAG edge, per the spec).
  3. reuse-count(premise) = in-degree of `premise` in the premise DAG, i.e.
     how many OTHER declarations reference it.
  4. Summary metrics: reuse histogram, never-reused fraction (long-tail
     share), concentration (Gini coefficient + Shannon entropy over the
     reuse-count distribution), and the top-10 most-reused declarations.
  5. Verdict: "signal" | "flat" | "corpus-too-small" (see THRESHOLD_
     DECLARATIONS below for why the corpus-too-small gate exists).

v1.1 changes from v1 (adversarial-review fix set -- see
`local-docs/todo/thesis-realization-roadmap.md` for the roadmap this
still implements; the fixes below only change HOW step 1/2 above are
computed, not the method):
  - Comments (`-- line`, nesting-aware `/- block -/` including `/-- doc
    -/`) and double-quoted string literals are stripped from each file's
    text BEFORE declaration extraction and BEFORE reference matching (see
    `strip_comments_and_strings`). This means: a name mentioned only in
    prose/comments/strings is never a reference; a `theorem`/`lemma`/`def`
    keyword written inside a doc comment never creates a phantom
    declaration; and because stripping happens before body-span splitting,
    a doc comment that precedes declaration N is correctly excluded from
    declaration N-1's body (both problems shared the same root cause: the
    v1 parser operated on raw, un-stripped text).
  - Declarations are keyed by `namespace`-qualified name (e.g.
    `Boole.Family.V0Helpers.dedup`, or a bare top-level name with no
    namespace). `namespace <Name> ... end <Name>` nesting is tracked with
    a scanner (`_namespace_prefix_breakpoints`); `section`s nest for
    `end`-matching but do not affect the qualified name. Two declarations
    with the same short name in different namespaces are now distinct
    nodes instead of silently colliding in one dict slot.
  - v1.1 conservative bare-name resolution rule (see `_resolve_reference`):
    a bare reference resolves ONLY when it is unambiguous -- either the
    bare name has exactly one qualified declaration corpus-wide, or (when
    corpus-wide ambiguous) exactly one candidate shares the referencing
    declaration's own namespace. A bare name that is still ambiguous after
    that produces NO edge, rather than guessing. This is intentionally
    conservative: v1.1 does not implement Lean's `open`/import resolution.
  - Dotted-access guard: a bare-name occurrence immediately preceded by
    `.` (e.g. `map` inside `List.map`) never counts as a reference to an
    unrelated corpus declaration of the same bare name.
  - Identifiers are captured/matched over their full extent, including
    unicode continuation characters (e.g. mathlib-style subscripts like
    `eval₂`) and primes (`foo'`), not just ASCII word characters. A
    declaration name is never silently truncated at the first non-ASCII
    character.
  - `build_premise_dag` is a tokenize-once pass: each declaration's body is
    scanned with exactly one `finditer`, and each extracted identifier
    token is resolved via O(1) dict lookups against a bare-name index --
    not, as in v1, one `re.search` per (declaration, OTHER declaration
    name) pair. This makes the whole pass ~O(total corpus text size)
    rather than O(declaration_count^2 * average body length).

Known v1.1 limitations (still name-reference parsing, not real Lean
dependency extraction):
  - No `open`/import resolution: an ambiguous bare name (same short name
    in >=2 namespaces, referenced from outside all of them) is dropped
    rather than resolved, which can under-count genuine references that a
    real Lean elaborator would resolve via `open`.
  - Declaration bodies are delimited by the next `theorem`/`lemma`/`def`
    keyword textually, not by an actual parse of Lean syntax (multi-line
    strings inside a body are handled by the comment/string stripper, but
    other syntax like `#eval`/`macro`/`attribute` blocks are not modeled).
  - Full Lean dependency extraction (e.g. via `#print axioms` / the
    elaborator's environment) is a later refinement; this is intentionally
    the cheap v1/v1.1 measurement (see roadmap Method step 2).

Non-goals (see roadmap "Non-goals"): no on-chain reward, no conjecture-
market code, no premise-DAG consensus artifact, no NL->Lean. Pure offline
measurement, stdlib only.
"""
from __future__ import annotations

import argparse
import bisect
import json
import math
import re
import sys
from collections import Counter
from pathlib import Path
from typing import NamedTuple

# Below ~50 declarations, concentration statistics (Gini, entropy, "top-10%
# holds X% of edges") are not meaningful -- too few data points for the
# distribution shape to say anything reliable. This is the corpus-too-small
# gate from the T0 spec ("(c) corpus-too-small (n < threshold -- define
# it)"). Below this size the verdict is always "corpus-too-small",
# regardless of how concentrated the (statistically meaningless) reuse
# counts happen to look.
THRESHOLD_DECLARATIONS = 50

# "signal" criterion: the top TOP_FRACTION of declarations (by reuse count)
# must hold at least SIGNAL_HEAD_SHARE of all premise edges. This encodes
# "a small head dominates reuse" (roadmap Method step 5(a)) as a concrete,
# checkable rule.
TOP_FRACTION = 0.10
SIGNAL_HEAD_SHARE = 0.50

# Declaration keyword line, optionally preceded by an attribute
# (`@[reducible]`, `@[simp]`, ...) and/or modifiers (`private`, `protected`,
# `noncomputable`). Matches at the start of a (possibly indented) line so
# tactic-block lines like `have h := foo` are never mistaken for a
# declaration. The name is captured over its FULL extent -- `[^\W\d]`
# (a "word" char that is not a digit) for the required first character,
# then `[\w']*` for the rest -- so unicode continuation characters (e.g.
# mathlib-style subscripts like the trailing digit of `eval₂`, which
# Python's unicode-aware `\w` already matches) and Lean's prime suffix
# (`foo'`) are never truncated away.
_DECLARATION_RE = re.compile(
    r"(?m)^[ \t]*(?:@\[[^\]]*\]\s*)?"
    r"(?:private\s+|protected\s+|noncomputable\s+)*"
    r"(theorem|lemma|def)\s+([^\W\d][\w']*)"
)

# A `namespace <Name>` / `section [Name]` / `end [Name]` line, on already
# comment/string-stripped text (see `strip_comments_and_strings`) so that a
# mention of these keywords in prose/comments/strings is never mistaken for
# a real scope boundary. Anchored to the start of a (possibly indented)
# line, mirroring `_DECLARATION_RE`; the `\b` after the keyword rejects
# identifiers that merely start with "namespace"/"section"/"end" (e.g.
# `endHelper`). The name may be dotted (`namespace Boole.Family.V0Helpers`).
_NAMESPACE_LINE_RE = re.compile(
    r"(?m)^[ \t]*(namespace|section|end)\b[ \t]*"
    r"([^\W\d][\w']*(?:\.[^\W\d][\w']*)*)?"
)


class Declaration(NamedTuple):
    name: str
    kind: str
    file: str
    body: str
    namespace: str
    qualified_name: str


def find_lean_files(corpus_dirs: list[Path]) -> list[Path]:
    """Recursively collect `.lean` files under each corpus directory."""
    files: set[Path] = set()
    for corpus_dir in corpus_dirs:
        corpus_dir = Path(corpus_dir)
        if corpus_dir.is_dir():
            files.update(corpus_dir.rglob("*.lean"))
    return sorted(files)


def strip_comments_and_strings(text: str) -> str:
    """Strip Lean line comments (`--` to end of line), NESTING-aware block
    comments (`/- ... -/`, where a `/-- ... -/` doc comment is just a block
    comment that happens to start with an extra `-`), and double-quoted
    string literals (with `\\"` escape handling) from `text`.

    This is a hand-rolled scanner, not a single regex: Lean block comments
    NEST (`/- a /- b -/ c -/` is ONE comment, not two -- "c" is still
    inside it), and a regex without manual depth tracking cannot express
    that (a non-greedy `/-.*?-/` stops at the first `-/`, prematurely
    ending the comment and leaking the "still nested" remainder as if it
    were real code).

    Every stripped character is replaced by a single space; embedded
    newlines are always preserved as-is. This keeps line numbers and
    character offsets in the returned text identical to the original, so
    declaration body spans (computed from regex match offsets on the
    stripped text) still line up with real source positions.
    """
    n = len(text)
    out = list(text)
    i = 0
    block_depth = 0
    while i < n:
        two = text[i : i + 2]

        if block_depth > 0:
            if two == "/-":
                block_depth += 1
                out[i] = out[i + 1] = " "
                i += 2
                continue
            if two == "-/":
                block_depth -= 1
                out[i] = out[i + 1] = " "
                i += 2
                continue
            if text[i] != "\n":
                out[i] = " "
            i += 1
            continue

        if two == "/-":
            block_depth = 1
            out[i] = out[i + 1] = " "
            i += 2
            continue

        if two == "--":
            j = i
            while j < n and text[j] != "\n":
                out[j] = " "
                j += 1
            i = j
            continue

        if text[i] == '"':
            out[i] = " "
            i += 1
            while i < n:
                if text[i] == "\\" and i + 1 < n:
                    out[i] = out[i + 1] = " "
                    i += 2
                    continue
                if text[i] == '"':
                    out[i] = " "
                    i += 1
                    break
                if text[i] != "\n":
                    out[i] = " "
                i += 1
            continue

        i += 1

    return "".join(out)


def _namespace_prefix_breakpoints(text: str) -> tuple[list[int], list[str]]:
    """Scan `namespace`/`section`/`end` lines (on already-stripped `text`)
    and return, as two parallel lists sorted by offset, the namespace-
    qualification prefix in effect from each offset onward (until the next
    breakpoint). `section`s open a scope for `end`-matching purposes only
    -- per the v1.1 spec, sections do NOT contribute to the qualification
    prefix, only `namespace`s do. An `end` (with or without a name) pops
    the innermost open scope; a stray `end` with nothing open is ignored.
    """
    stack: list[tuple[str, str | None]] = []
    offsets = [0]
    prefixes = [""]
    for match in _NAMESPACE_LINE_RE.finditer(text):
        keyword, name = match.group(1), match.group(2)
        if keyword in ("namespace", "section"):
            stack.append((keyword, name))
        elif stack:
            stack.pop()
        prefix = ".".join(scope_name for kind, scope_name in stack if kind == "namespace" and scope_name)
        offsets.append(match.end())
        prefixes.append(prefix)
    return offsets, prefixes


def _prefix_at(offsets: list[int], prefixes: list[str], offset: int) -> str:
    """Namespace-qualification prefix in effect at `offset` (the most
    recent breakpoint at or before `offset`)."""
    index = bisect.bisect_right(offsets, offset) - 1
    return prefixes[max(index, 0)]


def parse_declarations(files: list[Path]) -> dict[str, Declaration]:
    """Enumerate `theorem`/`lemma`/`def` declarations and their source body.

    Comments and string literals are stripped BEFORE declaration
    extraction (see `strip_comments_and_strings`), so a `theorem ...` line
    written inside a doc comment never creates a phantom declaration.

    A declaration's body spans from its own signature to the start of the
    next declaration in the same file (or end of file), computed on the
    STRIPPED text -- so a doc comment that precedes declaration N is
    excluded from declaration N-1's body (its content is already blank,
    regardless of which body span it falls into).

    Declarations are keyed by their `namespace`-qualified name (e.g.
    `Boole.Family.V0Helpers.dedup`; a bare top-level name is its own key).
    `namespace <Name> ... end <Name>` nesting is tracked with a scanner
    (see `_namespace_prefix_breakpoints`), so two declarations with the
    same short name in different namespaces are distinct dict entries
    instead of silently colliding.
    """
    declarations: dict[str, Declaration] = {}
    for path in files:
        raw_text = path.read_text(encoding="utf-8")
        text = strip_comments_and_strings(raw_text)
        offsets, prefixes = _namespace_prefix_breakpoints(text)
        matches = list(_DECLARATION_RE.finditer(text))
        for index, match in enumerate(matches):
            kind, name = match.group(1), match.group(2)
            start = match.start()
            end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
            body = text[start:end]
            namespace = _prefix_at(offsets, prefixes, start)
            qualified_name = f"{namespace}.{name}" if namespace else name
            declarations[qualified_name] = Declaration(
                name=name,
                kind=kind,
                file=str(path),
                body=body,
                namespace=namespace,
                qualified_name=qualified_name,
            )
    return declarations


# Identifier token: any run of Python's unicode-aware `\w` (letters,
# digits, underscore -- this already covers mathlib-style subscripts like
# the trailing digit of `eval₂`) plus `'` (Lean's prime suffix, e.g.
# `foo'`). A single greedy scan captures each name as a WHOLE maximal run
# -- `foo'_aux` is always one token, never split into `foo'` + `_aux` --
# so no separate boundary lookaround is needed for prime/unicode safety.
_TOKEN_RE = re.compile(r"[\w']+")


def _resolve_reference(candidates: list[Declaration], referencer_namespace: str) -> Declaration | None:
    """v1.1 conservative bare-name resolution rule (no `open`/import
    resolution modeled): a bare reference resolves ONLY when it is
    unambiguous.
      1. If the bare name has exactly one qualified declaration
         corpus-wide, resolve to it (regardless of the referencer's own
         namespace).
      2. Otherwise (corpus-wide ambiguous), resolve to the SAME-namespace
         candidate if exactly one candidate shares the referencer's own
         namespace -- local names shadow the corpus-wide pool, similar to
         how Lean resolves names inside their own namespace before falling
         back to `open`ed/fully-qualified lookups.
      3. Otherwise, still ambiguous -- return None (no edge). Guessing
         between multiple same-named declarations in unrelated namespaces
         would be a false positive, so v1.1 deliberately drops the
         reference instead of picking one arbitrarily.
    """
    if len(candidates) == 1:
        return candidates[0]
    same_namespace = [c for c in candidates if c.namespace == referencer_namespace]
    if len(same_namespace) == 1:
        return same_namespace[0]
    return None


def build_premise_dag(declarations: dict[str, Declaration]) -> dict[str, set[str]]:
    """For each declaration, the set of OTHER corpus declarations it
    references. Self-references are excluded. Multiple textual references
    to the same premise collapse to a single DAG edge (a set, not a
    multiset).

    Tokenize-once design: each declaration's (already comment/string-
    stripped) body is scanned with exactly ONE `finditer` pass to extract
    identifier tokens, each resolved via an O(1) dict lookup against a
    bare-name index (see `_resolve_reference`) -- not, as in v1, one
    `re.search` per (declaration, OTHER declaration name) pair. This makes
    the whole pass ~O(total corpus text size) rather than
    O(declaration_count^2 * average body length).

    A token immediately preceded by `.` is a dotted/namespaced access
    (e.g. `map` inside `List.map`) and never counts as a reference to an
    unrelated corpus declaration of the same bare name.
    """
    bare_index: dict[str, list[Declaration]] = {}
    for decl in declarations.values():
        bare_index.setdefault(decl.name, []).append(decl)

    premise_dag: dict[str, set[str]] = {}
    for qualified_name, decl in declarations.items():
        premises: set[str] = set()
        body = decl.body
        for match in _TOKEN_RE.finditer(body):
            start = match.start()
            if start > 0 and body[start - 1] == ".":
                continue  # dotted-access guard: `X.token` is not a reference to `token`
            candidates = bare_index.get(match.group(0))
            if not candidates:
                continue
            resolved = _resolve_reference(candidates, decl.namespace)
            if resolved is None or resolved.qualified_name == qualified_name:
                continue
            premises.add(resolved.qualified_name)
        premise_dag[qualified_name] = premises
    return premise_dag


def total_premise_edges(premise_dag: dict[str, set[str]]) -> int:
    return sum(len(premises) for premises in premise_dag.values())


def compute_reuse_counts(premise_dag: dict[str, set[str]]) -> dict[str, int]:
    """reuse-count(premise) = in-degree in the premise DAG: how many OTHER
    declarations list `premise` as one of their premises."""
    reuse_counts: dict[str, int] = {name: 0 for name in premise_dag}
    for premises in premise_dag.values():
        for premise in premises:
            reuse_counts[premise] += 1
    return reuse_counts


def reuse_histogram(reuse_counts: dict[str, int]) -> dict[int, int]:
    """reuse-count -> number of declarations with that reuse-count."""
    return dict(Counter(reuse_counts.values()))


def never_reused_fraction(reuse_counts: dict[str, int]) -> float:
    """Long-tail share: fraction of declarations with reuse-count 0."""
    if not reuse_counts:
        return 0.0
    never_reused = sum(1 for count in reuse_counts.values() if count == 0)
    return never_reused / len(reuse_counts)


def top_reused(reuse_counts: dict[str, int], limit: int = 10) -> list[tuple[str, int]]:
    """Top-N most-reused declarations, ties broken alphabetically by name
    for deterministic output."""
    ranked = sorted(reuse_counts.items(), key=lambda item: (-item[1], item[0]))
    return ranked[:limit]


def gini_coefficient(values: list[int]) -> float:
    """Population Gini coefficient (mean absolute difference / (2 * n *
    mean)) over the reuse-count distribution. 0 = perfectly uniform reuse,
    approaching (n-1)/n = maximally concentrated reuse. Defined as 0.0 for
    an empty or all-zero distribution (no inequality to measure)."""
    n = len(values)
    if n == 0:
        return 0.0
    total = sum(values)
    if total == 0:
        return 0.0
    mean = total / n
    diff_sum = sum(abs(a - b) for a in values for b in values)
    return diff_sum / (2 * n * n * mean)


def shannon_entropy_bits(values: list[int]) -> float:
    """Shannon entropy (base 2, in bits) of the reuse-count distribution
    treated as an unnormalized frequency table. Low entropy = a few
    declarations account for most reuse; high entropy = reuse is spread
    evenly. Defined as 0.0 when there is no reuse to measure."""
    total = sum(values)
    if total <= 0:
        return 0.0
    entropy = 0.0
    for value in values:
        if value > 0:
            probability = value / total
            entropy -= probability * math.log2(probability)
    return entropy


def compute_verdict(declaration_count: int, reuse_counts: dict[str, int], total_edges: int) -> dict:
    """Verdict logic (roadmap Method step 5):
      (a) "signal"            -- n >= THRESHOLD_DECLARATIONS and a small
                                  head (top TOP_FRACTION of declarations)
                                  holds >= SIGNAL_HEAD_SHARE of all edges.
      (b) "flat"               -- n >= THRESHOLD_DECLARATIONS but reuse is
                                  not concentrated (or there is no reuse at
                                  all).
      (c) "corpus-too-small"  -- n < THRESHOLD_DECLARATIONS: concentration
                                  statistics are not meaningful yet.
    """
    criteria = {
        "thresholdDeclarations": THRESHOLD_DECLARATIONS,
        "topFraction": TOP_FRACTION,
        "signalHeadShare": SIGNAL_HEAD_SHARE,
        "topK": None,
        "headShare": None,
    }

    if declaration_count < THRESHOLD_DECLARATIONS:
        return {"verdict": "corpus-too-small", **criteria}

    if total_edges == 0:
        return {"verdict": "flat", **criteria}

    # round-half-up (not Python's round(), which uses banker's rounding /
    # round-half-to-even and would give topK=6 for n=65 instead of 7).
    top_k = max(1, math.floor(declaration_count * TOP_FRACTION + 0.5))
    sorted_counts = sorted(reuse_counts.values(), reverse=True)
    head_edges = sum(sorted_counts[:top_k])
    head_share = head_edges / total_edges
    criteria["topK"] = top_k
    criteria["headShare"] = head_share

    verdict = "signal" if head_share >= SIGNAL_HEAD_SHARE else "flat"
    return {"verdict": verdict, **criteria}


def run_experiment(corpus_dirs: list[Path]) -> dict:
    """Run the full T0 reuse-signal experiment over `corpus_dirs` and
    return the report dict (also the shape emitted by `--json`)."""
    files = find_lean_files(corpus_dirs)
    declarations = parse_declarations(files)
    premise_dag = build_premise_dag(declarations)
    reuse_counts = compute_reuse_counts(premise_dag)
    edges = total_premise_edges(premise_dag)
    values = list(reuse_counts.values())
    verdict_info = compute_verdict(len(declarations), reuse_counts, edges)

    return {
        "corpusDirs": [str(d) for d in corpus_dirs],
        "sourceFileCount": len(files),
        "declarationCount": len(declarations),
        "declarations": sorted(declarations.keys()),
        "totalPremiseEdges": edges,
        "premisesByDeclaration": {name: sorted(premises) for name, premises in premise_dag.items()},
        "reuseCounts": reuse_counts,
        "histogram": reuse_histogram(reuse_counts),
        "neverReusedCount": sum(1 for c in values if c == 0),
        "neverReusedFraction": never_reused_fraction(reuse_counts),
        "giniCoefficient": gini_coefficient(values),
        "shannonEntropyBits": shannon_entropy_bits(values),
        "topReused": top_reused(reuse_counts, limit=10),
        "verdict": verdict_info["verdict"],
        "verdictCriteria": verdict_info,
    }


def format_human_report(report: dict) -> str:
    lines = [
        "T0 reuse-signal experiment report",
        "==================================",
        f"Corpus dirs:            {', '.join(report['corpusDirs']) or '(none)'}",
        f"Source files scanned:   {report['sourceFileCount']}",
        f"Declarations:           {report['declarationCount']}",
        f"Total premise edges:    {report['totalPremiseEdges']}",
        f"Never-reused:           {report['neverReusedCount']} "
        f"({report['neverReusedFraction'] * 100:.1f}% of declarations)",
        f"Gini coefficient:       {report['giniCoefficient']:.4f}",
        f"Shannon entropy (bits): {report['shannonEntropyBits']:.4f}",
        "",
        "Reuse histogram (reuse-count -> #declarations):",
    ]
    for count in sorted(report["histogram"]):
        lines.append(f"  {count}: {report['histogram'][count]}")

    lines.append("")
    lines.append("Top-10 most-reused declarations:")
    if report["topReused"]:
        for name, count in report["topReused"]:
            lines.append(f"  {name}: {count}")
    else:
        lines.append("  (no declarations)")

    criteria = report["verdictCriteria"]
    lines.append("")
    lines.append(f"Verdict: {report['verdict']}")
    lines.append(
        "  criteria: thresholdDeclarations="
        f"{criteria['thresholdDeclarations']}, topFraction={criteria['topFraction']}, "
        f"signalHeadShare={criteria['signalHeadShare']}, topK={criteria['topK']}, "
        f"headShare={criteria['headShare']}"
    )
    return "\n".join(lines) + "\n"


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="T0 offline experiment: measure whether reuse (in-degree "
        "of the premise DAG) is a concentrated, non-trivial signal over a "
        "corpus of Lean source files.",
    )
    parser.add_argument(
        "--corpus-dir",
        dest="corpus_dirs",
        action="append",
        metavar="DIR",
        help="Directory to scan recursively for .lean files. May be repeated.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit the machine-readable JSON report instead of the human-readable summary.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_arg_parser()
    args = parser.parse_args(argv)

    if not args.corpus_dirs:
        parser.error("at least one --corpus-dir is required")

    corpus_dirs = [Path(d) for d in args.corpus_dirs]
    report = run_experiment(corpus_dirs)

    if args.json:
        print(json.dumps(report, indent=2))
    else:
        print(format_human_report(report), end="")

    return 0


if __name__ == "__main__":
    sys.exit(main())
