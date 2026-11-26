"""Measure the pinned v6 text rules on fixed data without importing the registry."""

import argparse
import ast
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import sys
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[1]
CHECK = 'scripts/zk_registry/check.py'
NUMERALS = 'fixtures/submission-policy/numerals-v6.json'
POLICY5 = 'scripts/zk_registry/data/submission_policy_v5.json'
POLICY6 = 'scripts/zk_registry/data/submission_policy_v6.json'
INPUTS = {
    CHECK: '9d19b92c4e6722d152e3ba106b2758c63fb16324648152e5c52c233b46ddc08e',
    NUMERALS: '6ff47d0413ecd1786f84b1dfea9751ff29b5cf3d9ea98c6ef6f0318ffb39552c',
    POLICY5: '56f693d89a1a07751404e987457712bd4776f82aac921d6cf74bc329309aecc3',
    POLICY6: '999dc86304a54fd77bd1ff3dee7d967d7cb50a35ee4c053779f85c74bd8aba32',
}
LIBRARY = {
    'Init/Data/List/Basic.lean': '9d39c20a6de1e973e13490ec81a30672296f173e34014a0e8ba0109958534346',
    'Init/Data/Option/Basic.lean': 'bc690650cf1e4277cf83a418ec351bb79f6628cb41d9260a1979a496eaa2a16f',
    'Init/Data/Nat/Basic.lean': '92edcd298633bc57f70cf6c9cea73a3b6f984b91b3c8cc8fcc72f57bada0ee36',
    'Init/Data/Array/Basic.lean': '85b1fcc2eac430164656c02295d9cf5d3549826517a2a89083a7afae28b1e4f2',
}
# Header spellings at column one. Keep bare ASCII names and exclude qualified
# names, primed names, inline attributes and multiline headers from this sample.
HEADER = re.compile(
    r'^(?:(?:protected|private|noncomputable|partial) )*(?:def|abbrev|theorem) '
    r'([A-Za-z_][A-Za-z0-9_]*)(?=[ \t:(\[{]|$)', re.MULTILINE,
)
EXAMPLES = ('alternative', 'is_unsafe', 'expand_macro', 'concrete_syntax',
            'current_axiom', 'conservative', 'count', 'maxValue')
CONTEXTS = {
    'declaration': 'def {name} : Nat := 0',
    'reference': 'example ( {name} : Nat) : Nat := {name}',
    'parenthesized_reference': 'example ( {name} : Nat) : Nat := ({name})',
    'binder': 'example ({name} : Nat) : Nat := {name}',
}


def pinned(path, sha):
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != sha:
        raise ValueError(f'input digest mismatch: {path}')
    return data


def pure_checker(source, policy):
    # Load only these definitions from the digest-checked source. In particular
    # no module import, top-level file I/O or registry stage is executed.
    definitions = {
        'text_pattern', 'numeral_end', 'identifier_interiors', 'LeftBoundary',
        'forbidden_attributes', 'output_option_positions', 'forbidden_name_positions',
        'forbidden_scan',
    }
    constants = {'_AFTER_LAYOUT', '_ID_PART', '_ASCII_IDENT', '_ASCII_WORD',
                 '_DECIMAL_DIGITS', '_RADIX_DIGITS'}
    if b'def attribute_comment_lines(' in source:
        definitions.add('attribute_comment_lines')
    if b'def scan_attributes(' in source:
        definitions.add('scan_attributes')
    body = []
    found = set()
    for node in ast.parse(source).body:
        if isinstance(node, (ast.FunctionDef, ast.ClassDef)) and node.name in definitions:
            found.add(node.name)
            body.append(node)
        elif isinstance(node, ast.Assign) and len(node.targets) == 1:
            target = node.targets[0]
            if isinstance(target, ast.Name) and target.id in constants:
                found.add(target.id)
                body.append(node)
    if found != definitions | constants:
        raise ValueError('incomplete scan implementation')
    namespace = {'current_text_policy': lambda: policy, 're': re}
    exec(compile(ast.Module(body=body, type_ignores=[]), CHECK, 'exec'), namespace)
    return namespace['forbidden_scan']


def columns(policy):
    items = [(rule['text'] + ('item' if rule['boundary'] == 'prefix' else ''), rule['label'])
             for rule in policy['patterns']]
    items.extend(('set_option ' + opt + ' 10', 'limit-override')
                 for opt in ('maxHeartbeats', 'maxRecDepth'))
    items.extend(('set_option ' + root + '.probe true', 'output-option')
                 for root in policy['output_options']['forbidden_roots'])
    items.extend([('attribute [not_allowlisted]', 'forbidden-attribute'),
                  ('@[not_allowlisted]', 'forbidden-attribute')])
    return items


def measure_text(rows, policy, scan):
    items = columns(policy)
    if len(rows) != 40 or len({r['name'] for r in rows}) != 40:
        raise ValueError('unexpected numeral inventory')
    if len(items) != 53 or len({text for text, _ in items}) != 53:
        raise ValueError('unexpected policy inventory')
    table = []
    counts = [0] * len(items)
    for row in rows:
        bits = []
        for i, (text, label) in enumerate(items):
            diagnostics = scan([('helper', '\n' + row['literal'] + text)])
            prefix = f'{label} in helper (line 2 of that part)'
            caught = any(d == prefix or d.startswith(prefix + ' [') for d in diagnostics)
            counts[i] += int(caught)
            bits.append('1' if caught else '0')
        table.append({'row': row['name'], 'expected_label_hits': ''.join(bits),
                      'v6_caught': bits.count('1')})
    return {
        'columns': [{'text': t, 'label': label, 'v6_caught': count}
                    for (t, label), count in zip(items, counts, strict=True)],
        'rows': table, 'cells': len(items) * len(rows), 'v6_caught': sum(counts),
        'lean_parsed': None, 'v5_missed': None, 'lean_parsed_and_v6_missed': None,
    }


def measure_names(names, v5, v6):
    counts = {}
    differences = []
    for context, template in CONTEXTS.items():
        totals = Counter({'v5_hit': 0, 'v6_hit': 0, 'additional_hit': 0, 'removed_hit': 0})
        for name in sorted(names):
            source = template.format(name=name)
            before = v5([('helper', source)])
            after = v6([('helper', source)])
            totals['v5_hit'] += bool(before)
            totals['v6_hit'] += bool(after)
            totals['additional_hit'] += bool(after) and not before
            totals['removed_hit'] += bool(before) and not after
            if bool(after) != bool(before):
                differences.append({'name': name, 'context': context,
                                    'v5': before, 'v6': after})
        counts[context] = dict(totals)
    return {'names': len(names), 'cases': len(names) * len(CONTEXTS),
            'counts_by_context': counts, 'verdict_differences': differences}


def measure_hash_commands(policy, scan):
    symbol_hits = {}
    for rule in policy['patterns']:
        token = rule['text']
        if token.startswith('#'):
            expected = f"{rule['label']} in helper (line 1 of that part)"
            if token not in rule['label']:
                expected += f' [forbidden word: {token}]'
            symbol_hits[token] = {
                case: scan([('helper', token + suffix)]) == [expected]
                for case, suffix in (('spaced', ' item'), ('identifier', 'item'),
                                     ('digit', '1'), ('parenthesis', '(item)'))
            }
    return symbol_hits


class ScanningWork:
    """Logical source visits and interpreter line events for the refusal scan."""

    def __init__(self, scan):
        self.scan = scan
        self.visits = Counter(dict(iteration=0, indexing=0, find=0, count=0,
                                   prefix=0, regex=0, python=0))
        self.regex = Counter(dict(calls=0, matches=0, matched_characters=0))

    def run(self, source):
        work = self.visits
        regex = self.regex

        class Text(str):
            def __iter__(self):
                work['iteration'] += len(self)
                return super().__iter__()

            def __getitem__(self, key):
                work['indexing'] += len(range(*key.indices(len(self)))) if isinstance(key, slice) else 1
                return super().__getitem__(key)

            def find(self, sub, start=0, end=None):
                found = super().find(sub, start, end)
                first, last, _ = slice(start, end).indices(len(self))
                work['find'] += max(0, (last if found < 0 else found + len(sub)) - first)
                return found

            def __contains__(self, sub):
                return self.find(sub) != -1

            def count(self, sub, start=0, end=None):
                first, last, _ = slice(start, end).indices(len(self))
                work['count'] += max(0, last - first)
                return super().count(sub, start, end)

            def startswith(self, prefix, start=0, end=None):
                work['prefix'] += sum(map(len, prefix)) if isinstance(prefix, tuple) else len(prefix)
                return super().startswith(prefix, start, end)

        class Pattern:
            def __init__(self, pattern):
                self.pattern = pattern

            def finditer(self, text, start=0, end=None):
                end = len(text) if end is None else end
                work['regex'] += end - start
                regex['calls'] += 1
                for match in self.pattern.finditer(text, start, end):
                    self.record(match)
                    yield match

            @staticmethod
            def record(match):
                if match is not None:
                    regex['matches'] += 1
                    regex['matched_characters'] += match.end() - match.start()

            def search(self, text, start=0, end=None):
                end = len(text) if end is None else end
                match = self.pattern.search(text, start, end)
                regex['calls'] += 1
                self.record(match)
                work['regex'] += max(1, (end if match is None else match.end()) - start)
                return match

            def match(self, text, start=0, end=None):
                end = len(text) if end is None else end
                match = self.pattern.match(text, start, end)
                regex['calls'] += 1
                self.record(match)
                work['regex'] += 1 if match is None else max(1, match.end() - start)
                return match

            def fullmatch(self, text):
                work['regex'] += len(text)
                regex['calls'] += 1
                match = self.pattern.fullmatch(text)
                self.record(match)
                return match

        class Regex:
            ASCII = re.ASCII
            escape = staticmethod(re.escape)

            @staticmethod
            def compile(pattern, flags=0):
                return Pattern(re.compile(pattern, flags))

        filename = self.scan.__code__.co_filename

        def trace(frame, event, _):
            if frame.f_code.co_filename != filename:
                return None
            if event == 'line':
                work['python'] += 1
            return trace

        scope = self.scan.__globals__
        replacements = {name: Pattern(value) for name, value in scope.items()
                        if isinstance(value, re.Pattern)}
        replacements.update(re=Regex)
        replacements['_RADIX_DIGITS'] = {k: Pattern(p) for k, p in scope['_RADIX_DIGITS'].items()}
        previous_trace = sys.gettrace()
        with patch.dict(scope, replacements):
            sys.settrace(trace)
            try:
                lines, more = self.scan(Text(source))
            finally:
                sys.settrace(previous_trace)
        return {'input_characters': len(source), 'primitives': dict(work), 'regex': dict(regex),
                'operations': sum(work.values()), 'diagnostics': len(lines), 'more': more}


def attribute_comment_cases():
    yield 'block_prefix', 'attribute /- note -/ [noinline] probe', [1]
    yield 'line_prefix', '\nattribute -- note\n [noinline] probe', [2]
    yield 'allowed_name_with_comment', 'attribute /- note -/ [simp] probe', [1]
    yield 'command_list', 'attribute [simp /- note -/] probe', [1]
    yield 'inline_list', '@[simp /- note -/] def probe := 0', [1]
    yield 'list_with_raw_bracket', '@[simp /- note ] -/, not_allowlisted] def probe := 0', [1]
    yield 'line_opener_in_list', 'attribute [simp -- note\n] probe', [1]
    yield 'raw_string_text', 'def exampleText := "attribute /- prose -/ [simp]"', [1]
    yield 'spaced_allowed', 'attribute   [simp] probe', []
    yield 'inline_allowed', '@[simp] def probe := 0', []
    yield 'comment_after_list', 'attribute [simp] probe -- note', []
    yield 'intervening_word', 'attribute probe /- note -/ [simp]', []
    yield 'longer_identifier', 'myattribute /- note -/ [simp]', []
    yield 'prefix_without_bracket', 'attribute -- note', []


def attribute_delimiter_cases():
    yield 'inline_escaped_close', '@[specialize «a]», noinline] def probe := 0', 'forbidden-name-character', 1
    yield 'command_escaped_close', 'attribute [specialize «a]», noinline] probe', 'forbidden-name-character', 1
    yield 'escaped_open', '\ndef «example[» := 0', 'forbidden-name-character', 2
    yield 'nested_later_forbidden', '@[specialize [probe], not_allowlisted] def probe := 0', 'forbidden-attribute', 1
    yield 'inline_nested_comment', '@[specialize [probe] /- note -/] def probe := 0', 'attribute-comment', 1
    yield 'command_nested_comment', '\nattribute [specialize [probe] -- note\n] probe', 'attribute-comment', 2
    yield 'nested_mismatch', '@[specialize (probe] def probe := 0', 'forbidden-attribute', 1
    yield 'unclosed_nested', '@[specialize [probe]', 'forbidden-attribute', 1
    yield 'ordinary_specialize', '@[specialize probe] def probe := 0', None, None
    yield 'escaped_specialize', 'attribute [specialize «plain name»] probe', None, None
    yield 'nested_without_comment', '@[specialize [probe]] def probe := 0', None, None


def attribute_work_inputs():
    for size in (64, 128, 256, 512):
        yield 'prefixes', size, 'attribute -- note\n[simp] probe\n' * size
        yield 'lists', size, '@[simp /- note -/] def probe := 0\n' * size
        yield 'pending_prefixes', size, 'attribute /- note\n' * size + '[simp] probe'
        yield 'numeral_left_boundary', size, '1attribute /- note -/ [simp] probe\n' * size
        yield 'dense_list', size, 'attribute [' + 'simp /- note -/, ' * size + 'simp] probe'
        yield 'nested_lists', size, ('@[specialize ' + '[' * size + 'probe' + ']' * size
                                     + ' /- note -/] def probe := 0\n')


def measure_attribute_work(scan):
    report = {}
    for name, size, source in attribute_work_inputs():
        report.setdefault(name, []).append({'size': size, **ScanningWork(scan).run(source)})
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lean-source-root', type=Path, required=True)
    parser.add_argument('--hold', type=Path, required=True)
    parser.add_argument('--hold-schema', required=True)
    args = parser.parse_args()
    if not args.lean_source_root.is_absolute():
        parser.error('Lean source root must be absolute')
    if not args.hold.is_absolute():
        parser.error('chain hold path must be absolute')
    hold = json.loads(args.hold.read_bytes())
    if hold.get('schema') != args.hold_schema or hold.get('hold') is not False:
        parser.error('chain hold is not clear')

    inputs = {name: pinned(ROOT / name, sha) for name, sha in INPUTS.items()}
    names = set()
    per_file = {}
    for path, sha in LIBRARY.items():
        source = pinned(args.lean_source_root / path, sha).decode('utf-8')
        selected = set(HEADER.findall(source))
        per_file[path] = len(selected)
        names.update(selected)

    policy5, policy6 = (json.loads(inputs[path]) for path in (POLICY5, POLICY6))
    v5, v6 = (pure_checker(inputs[CHECK], p) for p in (policy5, policy6))
    attr = v6.__globals__['attribute_comment_lines']
    result = {
        'schema': 'registry-v6-text-measurement/1',
        'scope': 'offline text-policy evaluation', 'lean_processes_launched': 0,
        'input_sha256': INPUTS, 'library_sha256': LIBRARY,
        'text_matrix': measure_text(json.loads(inputs[NUMERALS]), policy6, v6),
        'hash_commands': measure_hash_commands(policy6, v6),
        'attribute_comment_controls': {
            name: {
                'expected_lines': lines,
                'match': attr(source) == (lines, False),
                'diagnostics': v6([('helper', source)]),
            }
            for name, source, lines in attribute_comment_cases()
        },
        'attribute_comment_work': measure_attribute_work(attr),
        'attribute_delimiter_controls': {
            name: {
                'expected_label': label,
                'expected_line': line,
                'diagnostics': v6([('helper', source)]),
            }
            for name, source, label, line in attribute_delimiter_cases()
        },
        'identifier_cases': {
            'templates': CONTEXTS, 'library_names_per_file': per_file,
            'library': measure_names(names, v5, v6),
            'selected_examples': list(EXAMPLES),
            'contrast_sample': measure_names(EXAMPLES, v5, v6),
        },
    }
    json.dump(result, sys.stdout, ensure_ascii=False, indent=2)
    sys.stdout.write('\n')


if __name__ == '__main__':
    main()
