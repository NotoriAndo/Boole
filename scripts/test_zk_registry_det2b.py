import json, re, sys, unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import check as C
class PolicyV5(unittest.TestCase):
    def test_shared_cases(self):
        corpus=json.loads((Path(__file__).resolve().parents[1]/'fixtures/submission-policy/cases-v5.json').read_text())
        for case in corpus['cases']:
            with self.subTest(case=case['name']), C.submission_policy(C.policy_descriptor(5)):
                actual=C.forbidden_scan([('part',case['source'])])
                expected=[(h['label'],h['line']) for h in case['hits']]
                self.assertEqual(sorted((h.split(' in ')[0],int(re.search(r'line (\d+)',h)[1])) for h in actual),sorted(expected))
    def test_historical_vocabulary(self):
        self.assertNotIn('output-command', C.FORBIDDEN_SETS[4])
        self.assertTrue({'output-command','output-option','forbidden-name-character'}<=set(C.FORBIDDEN_SETS[5]))

class GateDescriptor(unittest.TestCase):
    def test_new_store_records_current_policy(self):
        import tempfile, hashlib
        from zk_registry import service as S
        with tempfile.TemporaryDirectory() as root:
            registry=S.Registry.create(root,dict(entries=[],packages={},excluded={}))
            self.assertEqual(registry.status()['submission_policy'],dict(version=6,sha256=hashlib.sha256((Path(C.__file__).parent/'data/submission_policy_v6.json').read_bytes()).hexdigest()))


class PolicyV6(unittest.TestCase):
    fixtures = Path(__file__).resolve().parents[1] / 'fixtures/submission-policy'

    def test_v6_retains_the_previous_rule_inventory(self):
        with C.submission_policy(C.policy_descriptor(5)):
            previous = json.loads(json.dumps(C.current_text_policy()))
        with C.submission_policy(C.policy_descriptor(6)):
            current = C.current_text_policy().copy()
            self.assertEqual(current.pop('left_boundary'), 'lean-4.34.1-conservative-token-start')
            self.assertEqual(current.pop('attribute_comments'), 'refuse')
            previous['forbidden_name_characters'][1:1] = ['U+005B', 'U+005D']
            for versioned_field in ('version', 'word_boundary'):
                previous.pop(versioned_field)
                current.pop(versioned_field)
            for rule in previous['patterns']:
                if rule['text'] in ('#eval', '#exit'):
                    self.assertEqual(rule['boundary'], 'suffix')
                    rule['boundary'] = 'any'
            self.assertEqual(current, previous)
            self.assertIn('attribute-comment', C.FORBIDDEN_SETS[6])
            self.assertNotIn('attribute-comment', C.FORBIDDEN_SETS[5])
            self.assertIn('forbidden-name-character in proof (line 1 of that part)',
                          C.forbidden_scan([('proof', '«a\u2028b»')]))

    def test_numeric_contexts_cover_all_policy_labels(self):
        rows = json.loads((self.fixtures / 'numerals-v6.json').read_bytes())
        with C.submission_policy(C.policy_descriptor(6)):
            policy = C.current_text_policy()
            columns = [(rule['text'] + ('item' if rule['boundary'] == 'prefix' else ''), rule['label'])
                       for rule in policy['patterns']]
            columns.extend(('set_option ' + name + ' 10', 'limit-override')
                           for name in ('maxHeartbeats', 'maxRecDepth'))
            columns.extend(('set_option ' + root + '.probe true', 'output-option')
                           for root in policy['output_options']['forbidden_roots'])
            columns.extend([('attribute [not_allowlisted]', 'forbidden-attribute'),
                            ('@[not_allowlisted]', 'forbidden-attribute')])
            for row in rows:
                for spelling, label in columns:
                    with self.subTest(literal=row['name'], policy_text=spelling):
                        hits = C.forbidden_scan([('helper', '\n' + row['literal'] + spelling)])
                        self.assertTrue(any(hit.startswith(label + ' in helper (line 2 of that part)')
                                            for hit in hits), hits)

    def test_ordinary_identifiers_and_other_token_endings(self):
        with C.submission_policy(C.policy_descriptor(6)):
            for clean in ('standard_macro', 'compile2macro', '  macro2', 'my_axiom_lemma',
                          'Root.some_macro', "primed'.inner_macro", '10fieldmacro',
                          'a \t_localregister_name', 'set_option autoImplicit false'):
                with self.subTest(clean=clean):
                    self.assertEqual(C.forbidden_scan([('proof', clean)]), [])
            for ending in (')', '»', '"', '⁻¹', ':word', '::7', '(1 : Nat)'):
                with self.subTest(ending=ending):
                    self.assertIn('macro in proof (line 1 of that part)',
                                  C.forbidden_scan([('proof', ending + 'macro')]))

    def test_hash_command_boundary(self):
        with C.submission_policy(C.policy_descriptor(6)):
            for body in ('#eval 1+1', '#eval1+1', '#evalNat.succ 1'):
                with self.subTest(body=body):
                    self.assertEqual(C.forbidden_scan([('helper', body)]),
                                     ['#eval in helper (line 1 of that part)'])
            symbols = [(rule['text'], rule['label']) for rule in C.current_text_policy()['patterns']
                       if rule['text'].startswith('#')]
            self.assertEqual({name for name, _ in symbols},
                             {'#eval', '#exit', '#print', '#check', '#reduce', '#synth'})
            for token, label in symbols:
                expected = f'{label} in helper (line 1 of that part)'
                if token not in label:
                    expected += f' [forbidden word: {token}]'
                for after in (' item', 'item', '1', '(item)'):
                    with self.subTest(token=token, after=after):
                        self.assertEqual(C.forbidden_scan([('helper', token + after)]), [expected])
            for name in ('macroX', 'run_cmdX', 'axiom2', 'maxRecDepthHint'):
                with self.subTest(name=name):
                    self.assertEqual(C.forbidden_scan([('helper', name)]), [])

    def test_raw_attribute_comment_refusals(self):
        from measure_registry_policy_v6 import attribute_comment_cases

        with C.submission_policy(C.policy_descriptor(6)):
            for name, source, lines in attribute_comment_cases():
                with self.subTest(name=name):
                    expected = [f'attribute-comment in helper (line {line} of that part)' for line in lines]
                    diagnostics = C.forbidden_scan([('helper', source)])
                    self.assertEqual([hit for hit in diagnostics if hit.startswith('attribute-comment ')], expected)
                    self.assertEqual(bool(diagnostics), bool(lines))

    def test_attribute_comment_scan_work_is_linear(self):
        from measure_registry_policy_v6 import measure_attribute_work

        with C.submission_policy(C.policy_descriptor(6)):
            groups = measure_attribute_work(C.attribute_comment_lines)
            limit = C.current_text_policy()['attribute_limits']['reported_lines']
        for name, rows in groups.items():
            with self.subTest(name=name):
                self.assertEqual([row['size'] for row in rows], [64, 128, 256, 512])
                for row in rows:
                    self.assertEqual(row['diagnostics'], 1 if name in ('dense_list', 'nested_lists') else limit)
                    self.assertEqual(row['primitives']['iteration'], row['input_characters'])
                    self.assertEqual(row['primitives']['find'], 0)
                    self.assertEqual(row['primitives']['count'], 0)
                    self.assertLessEqual(row['operations'], 28 * row['input_characters'])
                for smaller, larger in zip(rows, rows[1:]):
                    self.assertLessEqual(larger['operations'], 2.2 * smaller['operations'])

    def test_attribute_delimiters_and_escaped_names(self):
        from measure_registry_policy_v6 import attribute_delimiter_cases

        with C.submission_policy(C.policy_descriptor(6)):
            for name, source, label, line in attribute_delimiter_cases():
                with self.subTest(name=name):
                    hits = C.forbidden_scan([('helper', source)])
                    if label is None:
                        self.assertEqual(hits, [])
                    else:
                        self.assertIn(f'{label} in helper (line {line} of that part)', hits)

    def test_clean_historical_corpora_stay_clean(self):
        with C.submission_policy(C.policy_descriptor(6)):
            for version in (4, 5):
                cases = json.loads((self.fixtures / f'cases-v{version}.json').read_bytes())['cases']
                for case in cases:
                    if not case['hits']:
                        with self.subTest(version=version, name=case['name']):
                            self.assertEqual(C.forbidden_scan([('proof', case['source'])]), [])

    def test_choice_and_pin_are_bound_to_the_store(self):
        descriptor = C.policy_descriptor(5)
        self.assertEqual(descriptor['sha256'], '56f693d89a1a07751404e987457712bd4776f82aac921d6cf74bc329309aecc3')
        with C.submission_policy(descriptor):
            self.assertEqual(C.current_text_policy()['version'], 5)
            with C.submission_policy(None):
                self.assertEqual(C.current_text_policy()['version'], 4)
            self.assertEqual(C.current_policy_descriptor(), descriptor)
        self.assertEqual(C.current_policy_descriptor()['version'], 6)
        for invalid in ({**descriptor, 'version': 6}, {**descriptor, 'version': 7}):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError), C.submission_policy(invalid):
                self.fail('mixed or unknown descriptor was selected')

    def test_numeric_endpoints(self):
        for spelling, end in (('01_2)', 4), ('0x_Fa:', 5), ('0B1_0;', 5),
                              ('0O77,', 4), ('1.50E-2:', 7), ('3e+2;', 4),
                              ('7..10', 1), ('7.#', 2), ('7.a', None),
                              ('3E+', None), ('0x_', None), ('1_!', None)):
            with self.subTest(spelling=spelling):
                self.assertEqual(C.numeral_end(spelling, 0), end)

    def test_worker_uses_job_descriptor_and_historical_fallback(self):
        from unittest.mock import patch
        from zk_registry import service_check as worker
        for job, version in (({}, 5), ({'submission_policy': None}, 4),
                             ({'submission_policy': C.policy_descriptor(5)}, 5),
                             ({'submission_policy': C.policy_descriptor(6)}, 6)):
            with self.subTest(job=job), patch.object(worker, '_run',
                    side_effect=lambda *args, **kw: C.current_text_policy()['version']):
                self.assertEqual(worker.run(job, '', '', ''), version)

if __name__=='__main__': unittest.main()
