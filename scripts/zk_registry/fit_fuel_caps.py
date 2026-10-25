"""Fit conservative C8 envelopes from both OS measurements of the checked path."""
from __future__ import annotations
import math
from . import fuel_caps as F

MARGIN = 1.10
WALL_SECONDS = 1800
RSS_MIB = 11000


def upper_fit(points):
    """Nonnegative OLS slope, then lift the intercept above every observation.

    This is an upper fit to the measured cohort, not an adversarial memory proof.
    There is no negative-intercept or zero-intercept extrapolation shortcut.
    """
    if not points or any(not math.isfinite(x) or not math.isfinite(y) or x <= 0 or y < 0
                         for x, y in points):
        raise ValueError('invalid upper-fit observations')
    xmean = sum(x for x, _ in points) / len(points)
    ymean = sum(y for _, y in points) / len(points)
    variance = sum((x - xmean) ** 2 for x, _ in points)
    slope = max(0.0, sum((x - xmean) * (y - ymean) for x, y in points) / variance) if variance else 0.0
    intercept = max(0.0, max(y - slope * x for x, y in points))
    return dict(slope=slope, intercept=intercept, observations=len(points))


def fit(darwin, linux, environments):
    """Return measured caps or explicit exclusions; missing inputs fail closed."""
    if (darwin['complete'] != darwin['total'] or linux['complete'] != linux['total']
            or darwin['total'] != linux['total']):
        raise ValueError('incomplete calibration cohort')
    left = {r['problem']: r for r in darwin['rows']}
    right = {r['problem']: r for r in linux['rows']}
    if len(left) != darwin['total'] or left.keys() != right.keys():
        raise ValueError('calibration identity mismatch')
    qualified = []
    exclusions = []
    for problem, a in left.items():
        b = right[problem]
        if a['status'] != 'MEASURED' or b['status'] != 'MEASURED':
            exclusions.append(dict(problem=problem, reason='reference-path-unqualified',
                                   darwin_status=a['status'], linux_status=b['status']))
            continue
        # Includes every per-declaration vector, typed status and the full debit.
        if (a['reference'] != b['reference'] or a['counters'] != b['counters']
                or a['reference_export'] != b['reference_export']):
            raise ValueError('cross-OS own counter mismatch: ' + problem)
        environment = environments.get(a['environment'])
        if (not environment or environment.get('eligible') is not True
                or environment.get('certified_kernels') != ['lean', 'nanoda']
                or not {'Darwin-arm64', 'Linux-aarch64'} <= environment.get('profiles', {}).keys()):
            exclusions.append(dict(problem=problem, reason='environment-not-dual-certified'))
            continue
        if any(not isinstance(row.get('comparator'), dict)
               or row['comparator'].get('verdict') != 'PASS' for row in (a, b)):
            exclusions.append(dict(problem=problem, reason='gate-path-unqualified'))
            continue
        for row in (a, b):
            for field in ('seconds', 'peak_rss_mb'):
                value = row['comparator'].get(field)
                if (type(value) not in (int, float) or not math.isfinite(value)
                        or value < 0 or field == 'seconds' and value == 0):
                    raise ValueError('invalid Comparator measurement')
        qualified.append((a, b))
    # An empty cohort cannot generate an admission table or fabricated fit.
    if not qualified:
        return dict(authority=dict(version=1, factors=dict(lean=4, nanoda=4), references={},
                                   environments=environments, unknown_pool_eligible=False),
                    exclusions=exclusions, envelope=None)
    ratios = {}
    memory = {}
    overhead = 0.0
    identity_peak = 0.0
    for kernel in ('lean', 'nanoda'):
        observations = []
        ratios[kernel] = 0.0
        for a, b in qualified:
            units = a['reference']['units'][kernel]
            if type(units) is not int or units <= 0:
                raise ValueError('invalid measured fuel')
            for row in (a, b):
                check = row['timing'][kernel]['check_ns'] / 1e9
                wall = row['measurement'][kernel]['seconds']
                if not math.isfinite(check) or check <= 0 or check > wall + 0.1:
                    raise ValueError('invalid private timing')
                ratios[kernel] = max(ratios[kernel], check / units)
                identity_peak = max(identity_peak, row['identity']['peak_rss_mb'])
            observations.append((units, max(b['measurement'][kernel]['peak_rss_mib'],
                                            b['comparator']['peak_rss_mb'])))
        memory[kernel] = upper_fit(observations)
    for a, b in qualified:
        for row in (a, b):
            # Parsing, certified-state import/identity and process setup remain
            # explicit, even though only checked declarations spend kernel fuel.
            # Include the actual Comparator's entire wall time as additional
            # overhead. Its reference kernel work is deliberately counted
            # twice rather than subtracting an independently timed execution.
            extra = row['comparator']['seconds'] + row['identity']['seconds'] + sum(max(0.0, row['measurement'][k]['seconds'] -
                        row['timing'][k]['check_ns'] / 1e9) for k in ('lean', 'nanoda'))
            overhead = max(overhead, extra)
    references = {}
    caps = []
    for a, b in qualified:
        reference = dict(a['reference'])
        units = {k: 4 * n for k, n in reference['units'].items()}
        seconds = MARGIN * (overhead + sum(units[k] * ratios[k] for k in units))
        rss = MARGIN * max(identity_peak, *(memory[k]['intercept'] + memory[k]['slope'] * units[k]
                                          for k in units))
        projection = dict(seconds=seconds, rss_mib=rss, margin=MARGIN)
        if (seconds > WALL_SECONDS or rss > RSS_MIB
                or any(units[k] > F.STRUCTURAL[k] for k in units)):
            exclusions.append(dict(problem=a['problem'], reason='tier-required', projection=projection))
            continue
        reference['eligible'] = True
        key = F.digest(reference)
        references[key] = reference
        cap = {k: reference[k] for k in ('problem', 'environment', 'statement', 'own_declarations')}
        cap.update(version=1, reference=key, units=units)
        caps.append(dict(problem=a['problem'], fuel_cap=dict(cap=cap, sha256=F.digest(cap)), projection=projection))
    return dict(authority=dict(version=1, factors=dict(lean=4, nanoda=4), references=references,
                               environments=environments, unknown_pool_eligible=False),
                caps=caps, exclusions=exclusions,
                envelope=dict(margin=MARGIN, wall_seconds=WALL_SECONDS, rss_mib=RSS_MIB,
                              worst_seconds_per_unit=ratios, overhead_seconds=overhead,
                              linux_upper_rss=memory, maximum_identity_rss_mib=identity_peak,
                              comparator_wall_in_overhead=True,
                              scope='measured checked-own cohort; N7/M-FULL remains unqualified'))
