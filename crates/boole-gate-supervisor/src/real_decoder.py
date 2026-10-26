"""Private real-controller decoder. libzstd typed API, never CLI diagnostics.
The Rust controller authenticates the interpreter, executable and runtime before
launch; this source is embedded in that controller. This is not contract Python.
"""
import ctypes, hashlib, json, os, sys

def decode(lib, source, destination, size, expected=None):
    try:
        with open(source, 'rb') as f:
            data = f.read()
    except OSError:
        return 'unavailable', 'read', None, None
    try:
        z = ctypes.CDLL(lib)
        z.ZSTD_decompress.argtypes = (ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t)
        z.ZSTD_decompress.restype = ctypes.c_size_t
        z.ZSTD_isError.argtypes = (ctypes.c_size_t,)
        z.ZSTD_isError.restype = ctypes.c_uint
        z.ZSTD_getErrorCode.argtypes = (ctypes.c_size_t,)
        z.ZSTD_getErrorCode.restype = ctypes.c_uint
        out = ctypes.create_string_buffer(size)
        result = z.ZSTD_decompress(out, size, data, len(data))
        if z.ZSTD_isError(result):
            code = z.ZSTD_getErrorCode(result)
            if code == 22:
                return 'invalid-data', 'checksum', None, None
            if code in (10, 12, 14, 16, 20, 24, 30, 32, 70, 72):
                return 'invalid-data', 'frame', None, None
            return 'unavailable', 'resource' if code in (64, 66) else 'protocol', None, None
        if result != size:
            return 'invalid-data', 'frame', None, None
        raw = memoryview(out).cast('B')[:result]
        raw_hash = hashlib.sha256(raw).hexdigest()
        if expected is not None and raw_hash != expected:
            return 'invalid-data', 'digest', None, None
        try:
            fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, 'wb') as f:
                f.write(raw)
                f.flush()
                os.fsync(f.fileno())
        except OSError:
            return 'unavailable', 'write', None, None
        return 'ok', 'none', raw_hash, result
    except (MemoryError, OverflowError):
        return 'unavailable', 'resource', None, None
    except (OSError, AttributeError):
        return 'unavailable', 'tool', None, None

if __name__ == '__main__':
    lib, source, output, size, binding, expected = sys.argv[1:]
    value = json.loads(binding)
    status, cause, raw_sha, raw_bytes = decode(lib, source, output, int(size), None if expected == "package" else expected)
    value.update(status=status, cause=cause, raw_sha256=raw_sha, raw_bytes=raw_bytes)
    sys.stdout.write(json.dumps(value, ensure_ascii=True, separators=(',', ':')))
