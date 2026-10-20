"""Fail-closed Lean 4.34.1 literal boundaries for the submission pre-scan.

Mirrors Parser.Basic (identifiers, comments, literals), Init.Meta.Defs (id chars)
and Parser.StrInterpolation. Interpolated terms remain code, including nested
literals. This is a lexical classifier, not a replacement for Lean's parser.
"""
import re

RAW_START = re.compile(r'r#*"')


class UnparseableSource(ValueError):
    pass


def letter_like(c):
    v = ord(c)
    return (0x3b1 <= v <= 0x3c9 and v != 0x3bb or
            0x391 <= v <= 0x3a9 and v not in (0x3a0, 0x3a3) or
            any(a <= v <= b for a, b in [(0x3ca, 0x3fb), (0x1f00, 0x1ffe),
                (0x2100, 0x214f), (0x1d49c, 0x1d59f), (0x100, 0x17f)]) or
            0xc0 <= v <= 0xff and v not in (0xd7, 0xf7))


def id_first(c):
    return c.isascii() and (c.isalpha() or c == '_') or letter_like(c)


def id_rest(c):
    v = ord(c)
    return (id_first(c) or c.isascii() and (c.isdigit() or c in "'!?") or
            any(a <= v <= b for a, b in [(0x2080, 0x2089), (0x2090, 0x209c), (0x1d62, 0x1d6a)]) or v == 0x2c7c)


def blank_non_code(src):
    """Preserve positions/newlines; raise for incomplete or ambiguous constructs."""
    n = len(src)
    out = ['\n' if c == '\n' else ' ' for c in src]

    def emit(a, b):
        out[a:b] = src[a:b]

    def escape(i, string=False, interpolated=False):
        if i >= n:
            raise UnparseableSource('unfinished escape')
        c = src[i]
        if c in "\\\"'rnt" or interpolated and c == '{':
            return i + 1
        if c in 'xu':
            end = i + (3 if c == 'x' else 5)
            if end <= n and all(k in '0123456789abcdefABCDEF' for k in src[i+1:end]):
                return end
        if string and c == '\n':
            i += 1
            while i < n and src[i] in ' \t\r':
                i += 1
            if i == n or src[i] != '\n':
                return i
        raise UnparseableSource('invalid literal escape')

    def identifier(i):
        start = i
        while True:
            if src[i] == '«':
                end = src.find('»', i + 1)
                if end < 0:
                    raise UnparseableSource('unterminated identifier')
                i = end + 1
            else:
                i += 1
                while i < n and id_rest(src[i]):
                    i += 1
            if i + 1 < n and src[i] == '.' and (id_first(src[i+1]) or src[i+1] == '«'):
                i += 1
            else:
                emit(start, i)
                return i

    def interpolation_context(i):
        j = i - 1
        while j >= 0 and out[j] in ' \t\r\n':
            j -= 1
        end = j + 1
        while j >= 0 and id_rest(out[j]):
            j -= 1
        prefix = ''.join(out[j+1:end])
        if prefix.endswith('!'):
            if prefix not in {'s!', 'm!', 'f!', 'println!'}:
                raise UnparseableSource('unknown string syntax')
            return True
        return prefix in {
            'dbg_trace', 'throwError', 'throwNamedError', 'throwNamedErrorAt', 'logNamedError',
            'logNamedErrorAt', 'logNamedWarning', 'logNamedWarningAt'}

    def string(i, depth):
        interpolated = interpolation_context(i)
        i += 1
        while i < n:
            c = src[i]
            if c == '"':
                return i + 1
            if c == '\\':
                i = escape(i + 1, string=True, interpolated=interpolated)
            elif interpolated and c == '{':
                i = code(i + 1, True, depth + 1)
            elif c == '{':
                # Lean has contextual interpolation macros (e.g. throwErrorAt
                # ref "..."). Without parsing their argument grammar this
                # delimiter is ambiguous. Never silently blank executable terms.
                # Raw strings provide an unambiguous spelling for literal braces.
                raise UnparseableSource('contextual string interpolation')
            else:
                i += 1
        raise UnparseableSource('unterminated string')

    def code(i, close=False, depth=0):
        if depth > 128:
            raise UnparseableSource('lexical nesting')
        while i < n:
            c = src[i]
            if close and c == '}':
                return i + 1
            if src.startswith('--', i):
                end = src.find('\n', i + 2)
                i = n if end < 0 else end
            elif src.startswith('/-', i):
                nesting, i = 1, i + 2
                while i < n and nesting:
                    if src.startswith('/-', i):
                        nesting, i = nesting + 1, i + 2
                    elif src.startswith('-/', i):
                        nesting, i = nesting - 1, i + 2
                    else:
                        i += 1
                if nesting:
                    raise UnparseableSource('unterminated comment')
            elif c == 'r' and (match := RAW_START.match(src, i)):
                endmark = '"' + '#' * (len(match[0]) - 2)
                end = src.find(endmark, i + len(match[0]))
                if end < 0:
                    raise UnparseableSource('unterminated raw string')
                i = end + len(endmark)
            elif c == '«' or id_first(c):
                i = identifier(i)
            elif c == '»':
                raise UnparseableSource('unexpected identifier delimiter')
            elif c == '"':
                i = string(i, depth)
            elif c == "'":
                i += 1
                if i >= n:
                    raise UnparseableSource('unterminated character')
                i = escape(i + 1) if src[i] == '\\' else i + 1
                if i >= n or src[i] != "'":
                    raise UnparseableSource('invalid character literal')
                i += 1
            elif close and c == '{':
                emit(i, i + 1)
                i = code(i + 1, True, depth + 1)
                emit(i - 1, i)
            else:
                emit(i, i + 1)
                i += 1
        if close:
            raise UnparseableSource('unterminated interpolant')
        return i

    code(0)
    return ''.join(out)
