"""Gnark soundness/completeness contracts against already accepted S specifications.

Pure interface decoding is separate from propositions. Concrete witness evaluation,
decidability instances and IO live in modules outside the statement import closure.
No missing specification or human proof is synthesized by this generator.
"""

from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
import re

from . import check as C
from . import gates as G
from . import lean_runner as L
from . import package as P
from . import spec_problem as S

SCHEMA_VERSION = 'zk-registry-correctness-gnark-problem/v1'
GENERATOR_NAME = 'boole-zk-registry-correctness-gnark'
OPERATIONS = {
    'Field.Reduce': 'reduce', 'Field.ReduceStrict': 'reduce', 'Curve.Neg': 'curve_neg', 'Field.Modulus': 'modulus',
    'Field.ToBits': 'to_bits', 'Field.ToBitsCanonical': 'to_bits', 'Field.Mux': 'mux', 'Field.FromBits': 'from_bits',
    'Field.MulConst': 'mul_const', 'Field.MulNoReduce': 'mul_no_reduce', 'Field.Select': 'select',
    'Field.Div': 'div', 'Ext2.Add': 'ext2_add', 'Field.Zero': 'zero',
    'Field.AssertIsDifferent': 'assert_different', 'Field.MulMod': 'mul_mod',
    # The original registry observes these emulated outputs modulo q, including the small-field raw branch.
    'Field.Mul': 'mul_mod',
    'Element.Initialize': 'initialize', 'Field.Add': 'add', 'Field.Sub': 'sub',
    'Field.One': 'one', 'Curve.AssertIsEqual': 'curve_equal',
}
LEAN_OPTIONS = ['--tstack=32768', '-DmaxRecDepth=100000', '-DmaxHeartbeats=0', '-DsynthInstance.maxSize=100000']
LIMITS = dict(timeout_seconds=1800, rss_mb=12000)


def generator_files() -> list[str]:
    """Registry-owned C generator, runtime, schema and upstream public parameter catalogue."""
    return ['correctness_gnark.py', 'correctness_build.py', 'correctness_runtime.py',
            'schema/correctness_gnark_problem.schema.json', 'data/gnark_parameters_cfc7b2f9.json']


def generator_info() -> dict:
    """Hash the C implementation, accepted-spec packaging and unchanged production core."""
    paths = generator_files() + S.generator_files() + ['package.py', 'jsonschema_lite.py', 'gnark_r1cs.py',
            'gnark_lean_emit.py', 'ratchet_gnark.py', 'r1cs.py', 'check.py', 'gates.py', 'lean_runner.py',
            'lean/ZkReplay.lean']
    digest = hashlib.sha256()
    for name in sorted(paths):
        digest.update(name.encode() + b'\0' + P.sha256_file(str(Path(__file__).parent / name)).encode() + b'\n')
    return dict(name=GENERATOR_NAME, version='1.0', sources_sha256=digest.hexdigest())


BATTERY_FORMS = [
    "(repeat' intro); simp_all",
    "(repeat' intro); simp_all [Soundness, WordValue, Inputs, Output, Active, Selected]; omega",
    "(repeat' intro); simp_all [Soundness, WordValue, Inputs, Output, Active, Selected]; decide",
    "(repeat' intro); simp_all [Soundness, WordValue, Inputs, Output, Active, Selected]; norm_num at *",
    "(repeat' intro); simp_all [Soundness, WordValue, Inputs, Output, Active, Selected]; ring_nf at *; omega",
    "(repeat' intro); simp_all [Soundness, WordValue, Inputs, Output, Active, Selected, "
    "Model.Constraints, Model.Assumptions]; omega",
    "(repeat' intro); simp_all [Soundness, WordValue, Inputs, Output, Active, Selected, "
    "Model.Constraints, Model.Assumptions]; norm_num at *; ring_nf at *; simp_all",
]


class NoCompatibleSpec(ValueError):
    """An original unit cannot be bound to an existing accepted specification scope."""


def bind_registry(directory: str, parameters: dict, ident: str, model_directory: str | None = None) -> dict:
    """Reconstruct the exact original R1CS and bind a supported semantic interface, without solving."""
    from . import ratchet_gnark as RG
    folder = Path(directory)
    original = json.loads((folder / 'problem.json').read_text())
    errors = P.validate_problem(original, None if model_directory else directory)
    if errors:
        raise ValueError('Invalid original registry package: ' + '; '.join(errors[:5]))
    function = original['ids']['template']
    op = OPERATIONS.get(function)
    if op is None:
        raise NoCompatibleSpec('No accepted specification for this function')
    spec = RG.reference_spec(directory)
    measured = RG.measure_reference(directory, spec)
    source = (Path(model_directory or directory) / spec['model']['file']).read_text()
    names = {int(n): name for n, name in re.findall(r'^\* `w (\d+)`: `([^`]+)`', source, re.M)}
    model = measured['model']
    inputs = [dict(wire=wire, name=names.get(wire, 'unknown')) for wire in model.inputs]
    matches = [re.fullmatch(r'(?:T|B)=emparams\.(\w+)', value) for value in original['instantiation']['args']]
    field = next((match[1] for match in matches if match), None)
    if field is None and op == 'ext2_add':
        for package, parameter_name in [('fields_bls12381', 'BLS12381Fp'), ('fields_bn254', 'BN254Fp')]:
            if package in original['ids']['path']:
                field = parameter_name
    if field not in parameters:
        raise NoCompatibleSpec('No pinned parameter binding for original field/type scope')
    parameter = parameters[field]
    catalogue = json.loads((Path(__file__).parent / 'data/gnark_parameters_cfc7b2f9.json').read_text())
    if original['ids']['commit'] != catalogue['source_commit']:
        raise NoCompatibleSpec('Source version has no independently pinned upstream parameter catalogue')
    if catalogue['parameters'].get(field) != parameter:
        raise NoCompatibleSpec('Supplied field/radix parameters differ from the pinned upstream catalogue')
    groups, singles = {}, []
    for row in inputs:
        match = re.fullmatch(r'(?:secret|public) (.*)_Limbs_(\d+)', row['name'])
        if match:
            groups.setdefault(match[1], []).append((int(match[2]), row['wire']))
        else:
            singles.append(row['wire'])
    grouped = [dict(name=name, wires=[wire for _, wire in sorted(values)],
                    bits=parameter['bits_per_limb'], modulus=parameter['modulus']) for name, values in groups.items()]
    outputs = original['statement'].get('emulated_outputs', [])
    if any(group['modulus'] != parameter['modulus'] or group['bits'] != parameter['bits_per_limb']
           for group in outputs):
        raise NoCompatibleSpec('Output field/radix does not match pinned accepted specification parameters')
    decode = ('raw-integer' if op in ('modulus', 'mul_no_reduce', 'initialize', 'from_bits') else
              'native-bit-vector' if op == 'to_bits' else
              'acceptance-predicate' if op in ('assert_different', 'curve_equal') else 'mod-foreign-q')
    constant = 12
    if op == 'mul_const':
        literal = re.search(r'\.MulConst\([^\n]*,\s*big\.NewInt\((\d+)\)\s*\)', spec['wrapper'])
        if literal is None:
            raise NoCompatibleSpec('MulConst requires an original nonnegative literal constant')
        constant = int(literal[1])
    if op == 'mul_no_reduce' and not re.search(r'\.MulNoReduce\(&c\.\w+,\s*&c\.\w+\)', spec['wrapper']):
        raise NoCompatibleSpec('Accepted MulNoReduce specification is the fixed secret-input branch only')
    if op == 'initialize' and (len(grouped) != 1 or not re.search(r'\.Initialize\(', spec['wrapper'])):
        raise NoCompatibleSpec('Accepted Initialize specification is the already-limbed receiver branch only')
    unit = dict(id=ident, registry_package_id=original['package_id'], source=original['ids'],
                gnark_version=original['ids'].get('release', spec['gnark_version']),
                compiler=original['circuit']['compiler'],
                original_metadata_sha256=P.sha256_file(str(folder / 'problem.json')),
                wrapper_sha256=P.sha256_file(str(folder / 'evidence/wrapper.go')),
                harness_sha256=P.sha256_file(str(folder / 'evidence/harness_result.json')),
                model_module=spec['model']['module'], model_file=spec['model']['file'],
                model_sha256=spec['model']['sha256'],
                original_r1cs_sha256=measured['r1cs_sha256'], n_wires=model.r.n_wires,
                n_constraints=len(model.r.constraints), native_prime=str(model.r.prime), inputs=inputs,
                native_outputs=[dict(wire=wire, name=names.get(wire, 'unknown')) for wire in model.native_outputs],
                emulated_outputs=outputs, hints=original['evidence'].get('hints', {}),
                hint_semantics='free/untrusted original prover wires',
                model_assumptions=original['statement']['assumptions'],
                gnark_commitment_model=original['circuit'].get('gnark', {}), instantiation=original['instantiation'],
                standard_function=function, reference_operation=op, parameter_field=field,
                field_parameters=copy.deepcopy(parameter), input_groups=grouped, scalar_or_bit_inputs=singles,
                output_decoding=decode, reference_cost=measured['counts'],
                spec_width=len(model.native_outputs) if op == 'to_bits' else 4, spec_constant=constant,
                output_limb_bounds=op == 'reduce')
    try:
        validate_interface(unit)
    except ValueError as ex:
        raise NoCompatibleSpec(str(ex)) from None
    return unit


def concrete_input_premises(unit: dict, witness: list[int]) -> dict:
    """Recheck the approved contracts' input/domain predicates; do not compute or assume their outputs."""
    import math
    values, canonical = [], True
    for group in unit['input_groups']:
        limbs = [witness[wire] for wire in group['wires']]
        value = sum(limb * 2 ** (group['bits'] * index) for index, limb in enumerate(limbs))
        canonical = canonical and all(limb < 2 ** group['bits'] for limb in limbs) and value < int(group['modulus'])
        values.append(value)
    op = unit['reference_operation']
    bound = 4 if op == 'mux' else 2
    canonical = canonical and all(witness[wire] < bound for wire in unit['scalar_or_bit_inputs'])
    modulus = int(unit['field_parameters']['modulus'])
    domain = math.gcd(values[1] % modulus, modulus) == 1 if op == 'div' else True
    accepted = domain
    if op == 'assert_different':
        accepted = values[0] % modulus != values[1] % modulus
    if op == 'curve_equal':
        accepted = values[0] % modulus == values[2] % modulus and values[1] % modulus == values[3] % modulus
    return dict(CanonicalInputs=bool(canonical), SpecDomain=bool(domain), AcceptedData=bool(accepted), BindInputs=True)


def make_record(unit: dict, spec: dict, kind: str, files: list[dict], reference_type: str,
                nonvacuity: dict, battery: dict, env: dict, created_utc: str) -> dict:
    """Create an OPEN or FACT record only after the bound gates have completed."""
    if spec.get('status') != 'ACCEPTED-LOCAL' or P.validate_problem(spec):
        raise ValueError('C requires a validated ACCEPTED-LOCAL S package')
    status = classify(nonvacuity['status'], battery['attempts'], battery.get('checker_verdict'))
    if status not in ('OPEN', 'FACT'):
        raise ValueError('Non-issuable contract gate status: ' + status)
    text = theorem_template(unit, kind)
    checker = checker_record(unit, kind, files, reference_type)
    linked = dict(package_id=spec['package_id'], sha256=S.package_digest(spec), file='spec/problem.json',
                  specification_sha256=spec['specification']['sha256'])
    identity = S.package_digest(dict(binding=unit, spec=linked, property=kind))
    return dict(schema_version=SCHEMA_VERSION, kind='C', family='gnark', package_id='C/gnark/' + identity,
                status=status, property=kind, binding=copy.deepcopy(unit), binding_sha256=S.package_digest(unit),
                spec=linked, nonvacuity=copy.deepcopy(nonvacuity), battery=copy.deepcopy(battery),
                trivial=dict(structural=unit['reference_cost']['nonlinear'] < 3, battery=status == 'FACT'),
                statement=dict(file='ProofStatement.lean', module='Statements', propositions_file='Statements.lean',
                               bindings_file='Bindings.lean', theorem='solution', theorem_fqn=checker['theorem_fqn'],
                               text=text, imports=['Statements']), checker=checker, files=copy.deepcopy(files),
                env=copy.deepcopy(env), generator=dict(name=GENERATOR_NAME, version='1.0'), created_utc=created_utc)


def nat_list(values: list) -> str:
    """Render a fixed list of wire indices or values without inferring logical facts."""
    return '[' + ', '.join(map(str, values)) + ']'


def validate_interface(unit: dict) -> None:
    """Require complete input coverage, supported arities and original output widths."""
    inputs = [row['wire'] for row in unit['inputs']]
    grouped = [wire for group in unit['input_groups'] for wire in group['wires']]
    described = grouped + unit['scalar_or_bit_inputs']
    if len(inputs) != len(set(inputs)) or sorted(inputs) != sorted(described) or len(described) != len(set(described)):
        raise ValueError('Every original input must occur exactly once in the semantic interface')
    wires = inputs + [wire for group in unit['emulated_outputs'] for wire in group['wires']]
    wires += [row['wire'] for row in unit['native_outputs']]
    if any(not isinstance(wire, int) or wire < 0 or wire >= unit['n_wires'] for wire in wires):
        raise ValueError('Interface wire outside original model')
    op = unit['reference_operation']
    groups, singles = len(unit['input_groups']), len(unit['scalar_or_bit_inputs'])
    arity = {'reduce': 1, 'curve_neg': 2, 'modulus': 0, 'to_bits': 1, 'mux': 4,
             'mul_const': 1, 'mul_no_reduce': 2, 'select': 2, 'div': 2, 'ext2_add': 4,
             'zero': 0, 'assert_different': 2, 'mul_mod': 2, 'initialize': 1, 'add': 2,
             'sub': 2, 'one': 0, 'curve_equal': 4}
    if op == 'from_bits':
        if groups or singles != 4:
            raise ValueError('Accepted FromBits specification is fixed to four Boolean digits')
    elif op not in arity or groups != arity[op] or singles != (1 if op in ('select', 'mux') else 0):
        raise ValueError('Interface does not match accepted specification arity')
    if any(group['bits'] <= 0 or int(group['modulus']) <= 0 for group in unit['input_groups']):
        raise ValueError('Invalid original limb width or modulus')
    if any(str(group['modulus']) != str(unit['field_parameters']['modulus'])
           or group['bits'] != unit['field_parameters']['bits_per_limb'] for group in unit['input_groups']):
        raise ValueError('Input groups differ from the original field/radix parameters')
    if unit.get('output_limb_bounds', False) != (op == 'reduce'):
        raise ValueError('Reduce must retain the approved output-limb conclusions')


def render_modules(unit: dict) -> dict[str, str]:
    """Generate pure bindings, proposition-only statements and a separate concrete evaluator."""
    validate_interface(unit)
    ident, op = unit['id'], unit['reference_operation']
    namespace = unit['model_module'].rsplit('.', 1)[0]
    positions = {row['wire']: i for i, row in enumerate(unit['inputs'])}
    count = len(positions)
    terms, arguments = [], []
    for group in unit['input_groups']:
        slots = [positions[wire] for wire in group['wires']]
        value = f"decodeData data {nat_list(slots)} {group['bits']}"
        arguments.append(value)
        terms += [f"decide (data {i} < 2 ^ {group['bits']})" for i in slots]
        terms.append(f"decide ({value} < {group['modulus']})")
    for wire in unit['scalar_or_bit_inputs']:
        bound = 4 if op == 'mux' else 2
        terms.append(f'decide (data {positions[wire]} < {bound})')
    if op == 'from_bits':
        arguments = [f'data {positions[wire]}' for wire in unit['scalar_or_bit_inputs']]
    selector = f"data {positions[unit['scalar_or_bit_inputs'][0]]}" if op in ('mux', 'select') else '0'
    outputs = []
    for group in unit['emulated_outputs']:
        value = f"emValue w {nat_list(group['wires'])} {group['bits']}"
        if unit['output_decoding'] == 'mod-foreign-q':
            value = f"({value}) % {group['modulus']}"
        outputs.append(value)
    outputs += [f"(w {row['wire']}).val" for row in unit['native_outputs']]
    if unit['output_decoding'] == 'acceptance-predicate':
        outputs = ['1']
    canonical = ' && '.join(terms) or 'true'
    binds = ' && '.join(f'decide ((w {wire}).val = data {i})' for wire, i in positions.items()) or 'true'
    domain = 'true' if unit['output_decoding'] == 'acceptance-predicate' else '!(specResult data).isEmpty'
    accepted = ('decide (specResult data = [1])'
                if unit['output_decoding'] == 'acceptance-predicate' else 'domainData data')
    binding = f"""import {unit['model_module']}
import Candidate

set_option maxRecDepth 100000
set_option maxHeartbeats 0
set_option synthInstance.maxSize 100000

namespace {ident}
open {namespace}

abbrev InputData := Fin {count} → Nat
def inputWires : Array (Fin nWires) := #{nat_list(list(positions))}
def dataOf (w : Fin nWires → F) : InputData := fun i => (w inputWires[i.val]!).val
def decodeData (data : InputData) (slots : List (Fin {count})) (bits : Nat) : Nat :=
  slots.foldr (fun i acc => data i + 2 ^ bits * acc) 0
def canonicalData (data : InputData) : Bool := {canonical}
def CanonicalData (data : InputData) : Prop := canonicalData data = true
def CanonicalInputs (w : Fin nWires → F) : Prop := CanonicalData (dataOf w)
def assumptions (_w : Fin nWires → F) : Bool := true
def Assumptions (w : Fin nWires → F) : Prop := assumptions w = true
def specResult (data : InputData) : List Nat := Candidate.eval {unit['field_parameters']['modulus']} ({selector}) {unit['spec_width']} {unit['spec_constant']} {nat_list(arguments)}
def domainData (data : InputData) : Bool := {domain}
def SpecDomain (w : Fin nWires → F) : Prop := domainData (dataOf w) = true
def acceptedData (data : InputData) : Bool := {accepted}
def AcceptedData (data : InputData) : Prop := acceptedData data = true
def outputValues (w : Fin nWires → F) : List Nat := {nat_list(outputs)}
def binds (w : Fin nWires → F) (data : InputData) : Bool := {binds}
def BindInputs (w : Fin nWires → F) (data : InputData) : Prop := binds w data = true
"""
    bound_conclusion = ''
    if unit['output_limb_bounds']:
        bounds = ' && '.join(f"decide ((w {wire}).val < 2 ^ {group['bits']})"
                             for group in unit['emulated_outputs'] for wire in group['wires']) or 'true'
        binding += (f'def outputLimbsBounded (w : Fin nWires → F) : Bool :=\n  {bounds}\n'
                    'def OutputLimbsBounded (w : Fin nWires → F) : Prop := outputLimbsBounded w = true\n')
        bound_conclusion = ' ∧ OutputLimbsBounded w'
    binding += f'\nend {ident}\n'
    propositions = f"""import Bindings

namespace {ident}
open {namespace}

def Soundness : Prop := ∀ w : Fin nWires → F,
  Constraints w → Assumptions w → CanonicalInputs w → SpecDomain w →
  outputValues w = specResult (dataOf w){bound_conclusion}

def Completeness : Prop := ∀ data : InputData,
  CanonicalData data → AcceptedData data →
  ∃ w : Fin nWires → F, Constraints w ∧ Assumptions w ∧ BindInputs w data ∧
    outputValues w = specResult data{bound_conclusion}

end {ident}
"""
    evaluation = f"""import Bindings
import EvaluationSupport
import Lean

set_option maxRecDepth 100000
set_option maxHeartbeats 0
set_option synthInstance.maxSize 100000

def main (paths : List String) : IO Unit := do
  let output ← IO.getStdout
  for path in paths do
    let text ← IO.FS.readFile path
    let values := (text.splitOn "\\n").filter (fun s => !s.trimAscii.toString.isEmpty)
    let numbers := (values.map (fun s => s.trimAscii.toString.toNat!)).toArray
    let w : Fin {namespace}.nWires → {namespace}.F := fun i => (numbers[i.val]! : Nat)
    let data := {ident}.dataOf w
    let validLength := numbers.size == {namespace}.nWires && numbers.all (fun n => n < {namespace}.p)
    let constraints := validLength && decide ({namespace}.Constraints w)
    let assumptions := {ident}.assumptions w
    let canonical := {ident}.canonicalData data
    let defined := {ident}.domainData data
    let accepted := {ident}.acceptedData data
    let binds := {ident}.binds w data
    let outputMatches := decide ({ident}.outputValues w = {ident}.specResult data)
    let record := Lean.Json.mkObj [
      ("witness", Lean.Json.str path), ("valid_length", Lean.toJson validLength),
      ("Constraints", Lean.toJson constraints), ("Assumptions", Lean.toJson assumptions),
      ("CanonicalInputs", Lean.toJson canonical), ("SpecDomain", Lean.toJson defined),
      ("AcceptedData", Lean.toJson accepted), ("BindInputs", Lean.toJson binds),
      ("output_matches_diagnostic_not_premise", Lean.toJson outputMatches)]
    output.putStrLn record.compress
"""
    return {'Bindings.lean': binding, 'Statements.lean': propositions, 'Evaluation.lean': evaluation}


def evaluation_support(model_text: str, model_module: str) -> str:
    """Add executable decidability only to the isolated evaluation module, never a proposition import."""
    namespace = model_module.rsplit('.', 1)[0]
    pattern = r'def (Block\d+|Constraints) \(w : Fin nWires → F\) : Prop :=\n(.*?)(?=\n(?:/--|end))'
    definitions = [(match[1], match[2].strip()) for match in re.finditer(pattern, model_text, re.S)]
    if not definitions or definitions[-1][0] != 'Constraints':
        raise ValueError('Unsupported original model definition layout')
    output = [f'import {model_module}', f'namespace {namespace}', 'set_option maxRecDepth 100000',
              'set_option maxHeartbeats 0', 'set_option synthInstance.maxSize 100000']
    for name, body in definitions:
        output.append(f'instance (w : Fin nWires → F) : Decidable ({name} w) :=\n'
                      f'  inferInstanceAs (Decidable (\n{body}\n  ))')
    output.append(f'end {namespace}')
    return '\n\n'.join(output) + '\n'


def nonvacuity_status(rows: list[dict], kind: str, completed: bool) -> str:
    """Require a single concrete row satisfying every premise simultaneously; fail closed on incomplete evaluation."""
    if not completed:
        return 'UNMEASURED'
    keys = ['valid_length', 'Constraints', 'Assumptions', 'CanonicalInputs']
    keys += ['SpecDomain'] if kind == 'soundness' else ['AcceptedData', 'BindInputs']
    return 'NONVACUOUS' if any(all(row.get(key) is True for key in keys) for row in rows) else 'VACUOUS'


def classify(nonvacuity: str, attempts: list[dict], checker_verdict: str | None) -> str:
    """Only an accepted fixed-battery closure becomes FACT; an incomplete gate is never issuable."""
    if nonvacuity == 'VACUOUS':
        return 'VACUOUS'
    if nonvacuity != 'NONVACUOUS':
        return 'EXCLUDED'
    if not attempts or any(not isinstance(row, dict) for row in attempts):
        return 'EXCLUDED'
    if [row.get('index') for row in attempts] != list(range(1, len(attempts) + 1)) or len(attempts) > 7:
        return 'EXCLUDED'
    if any(row.get('closed') is True for row in attempts):
        if not all(row.get('completed') is True for row in attempts):
            return 'EXCLUDED'
        return 'FACT' if checker_verdict == 'PASS' else 'EXCLUDED'
    if len(attempts) != len(BATTERY_FORMS) or sorted(row['index'] for row in attempts) != list(range(1, 8)):
        return 'EXCLUDED'
    return 'OPEN' if all(row.get('completed') is True for row in attempts) else 'EXCLUDED'


def theorem_template(unit: dict, kind: str) -> str:
    """A reference-only proof hole; no correctness proof is generated."""
    name = 'Soundness' if kind == 'soundness' else 'Completeness'
    namespace = 'MProof.' + unit['id'] + name
    return (f'import Statements\n\nnamespace {namespace}\n\n'
            f'theorem solution : _root_.{unit["id"]}.{name} := by\n  sorry\n\nend {namespace}\n')


def qualify_battery(unit: dict, kind: str, form: str) -> str:
    """Bind the frozen seven-form frame one-to-one to the generated definitions; add no tuned tactic."""
    namespace = unit['model_module'].rsplit('.', 1)[0]
    ident = unit['id']
    complete = kind == 'completeness'
    mapping = {'Soundness': ident + ('.Completeness' if complete else '.Soundness'),
               'WordValue': ident + '.decodeData', 'Inputs': ident + '.dataOf', 'Output': ident + '.outputValues',
               'Active': ident + ('.CanonicalData' if complete else '.CanonicalInputs'),
               'Selected': ident + ('.AcceptedData' if complete else '.SpecDomain'),
               'Model.Constraints': namespace + '.Constraints', 'Model.Assumptions': ident + '.Assumptions'}
    pattern = r'\b(?:Model\.Constraints|Model\.Assumptions|Soundness|WordValue|Inputs|Output|Active|Selected)\b'
    text = re.sub(pattern, lambda match: mapping[match.group()], form)
    return text.replace('simp_all [', 'simp_all [' + ident + '.specResult, ', 1)


def checker_record(unit: dict, kind: str, files: list[dict], reference_type: str) -> dict:
    """Production-core checker configuration with exact type, source closure and bounded resources."""
    name = 'Soundness' if kind == 'soundness' else 'Completeness'
    return dict(statement_file='ProofStatement.lean', theorem='solution',
                theorem_fqn='MProof.' + unit['id'] + name + '.solution', lean_opts=LEAN_OPTIONS,
                files=files, reference_type_sha256=reference_type,
                replay_tool_sha256=P.sha256_file(G.REPLAY_TOOL), allowed_axioms=sorted(C.ALLOWED_AXIOMS),
                forbidden_tokens=C.FORBIDDEN_LABELS, limits=copy.deepcopy(LIMITS))


def check_solution(directory: str, proof: str, env: L.LeanEnv, work_root: str, timeout: int = 1800) -> dict:
    """Validate C admission and then delegate all proof checks to the unchanged production core."""
    problem = json.loads((Path(directory) / 'problem.json').read_text())
    errors = P.validate_problem(problem, directory)
    if problem.get('schema_version') != SCHEMA_VERSION or errors:
        return dict(verdict='ERROR', error=errors or ['Not a C package'], invalid=[], fail=[])
    if problem['env'] != env.pins():
        return dict(verdict='ERROR', error=['Full Lean/package environment pins differ'], invalid=[], fail=[])
    from . import correctness_runtime
    root = Path(work_root)
    root.mkdir(parents=True, exist_ok=True)
    limit = min(timeout, problem['checker']['limits']['timeout_seconds'])
    correctness_runtime.install(str(root / 'processes'), total_timeout=limit)
    source = Path(proof).read_bytes()
    source_sha = hashlib.sha256(source).hexdigest()
    snapshot = root / ('submission-' + source_sha + '.lean')
    with snapshot.open('xb') as stream:
        stream.write(source)
    metadata_sha = P.sha256_file(str(Path(directory) / 'problem.json'))
    L.set_lean_limits(slots=1, rss_mb=problem['checker']['limits']['rss_mb'])
    result = C.check(directory, str(snapshot), env, timeout=limit, keep=True, work_root=work_root)
    if Path(proof).read_bytes() != source or P.sha256_file(str(Path(directory) / 'problem.json')) != metadata_sha:
        result['invalid'].append('submission or package metadata changed during checking')
    result['invalid'] += P.validate_problem(problem, directory)
    result['verdict'] = C.verdict_of(result)
    result['original_proof_file'] = proof
    result['source_snapshot_sha256'] = source_sha
    return result
