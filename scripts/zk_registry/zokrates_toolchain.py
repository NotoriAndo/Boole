"""ZoKrates compiler pinning: one pinned compiler per repository.

Two repositories, two release lines of the same project:

* ``zokrates/zokrates`` (the stdlib population) pins to its own commit, which is exactly the tag
  ``0.8.8``: a source build of the tag (:func:`build_source`; Cargo.lock at the pinned tag is the
  reproducibility pin, recorded by its own sha256), which emits the iden3/circom ``.r1cs``/``.wtns``
  formats directly with ``--r1cs``/``--circom-witness`` (:mod:`zokrates_r1cs` reads them unchanged).
  The official release binary (:func:`fetch_release`) is an available alternative for this platform
  and is used only to cross-check the source build's output, not as the pinned compiler itself.
* ``ethereum-oasis-op/baseline`` pins ZoKrates 0.6.1 (its own ``zok6.Dockerfile``: ``FROM
  zokrates/zokrates:0.6.1``), which predates both the arm64 macOS release asset (added in 0.7.10) and
  the R1CS export (added later still) and needs a nightly-only Rust feature its crate still implements.
  :func:`build_source` builds it natively from the pinned tag with an *already installed* stable rustc
  and ``RUSTC_BOOTSTRAP=1`` (no toolchain is installed by this call); its ``.ztf``/plain-text output is
  decoded by :mod:`zokrates_legacy`.
"""
from __future__ import annotations

import hashlib
import os
import platform
import stat
import subprocess
import tarfile
import urllib.request

RELEASE_URL = "https://github.com/Zokrates/ZoKrates/releases/download/{version}/{asset}"
USER_AGENT = "Mozilla/5.0 (compatible; boole-zk-registry-zokrates-det)"


def host_asset(version: str) -> str | None:
    """The release asset name for this host, or ``None`` when the release ships none."""
    mach = platform.machine()
    sysname = platform.system()
    if sysname == "Darwin":
        plat = "aarch64-apple-darwin" if mach == "arm64" else "x86_64-apple-darwin"
    elif sysname == "Linux":
        plat = "aarch64-unknown-linux-gnu" if mach in ("aarch64", "arm64") else "x86_64-unknown-linux-gnu"
    else:
        return None
    return f"zokrates-{version}-{plat}.tar.gz"


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


class ToolchainError(RuntimeError):
    pass


def fetch_release(version: str, dest_dir: str) -> dict:
    """Download, extract and verify the official release binary for ``version``; return its pin record
    (``{"version", "source", "binary", "binary_sha256", "asset", "stdlib"}``).  Cached under ``dest_dir``."""
    asset = host_asset(version)
    if asset is None:
        raise ToolchainError(f"no release asset for this host ({platform.system()}/{platform.machine()})")
    root = os.path.join(dest_dir, version)
    binary = os.path.join(root, "zokrates")
    stdlib = os.path.join(root, "stdlib")
    tarball = os.path.join(dest_dir, asset)
    if not os.path.isfile(binary):
        os.makedirs(root, exist_ok=True)
        if not os.path.isfile(tarball):
            req = urllib.request.Request(RELEASE_URL.format(version=version, asset=asset),
                                         headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(req, timeout=120) as r, open(tarball, "wb") as f:
                f.write(r.read())
        with tarfile.open(tarball) as tf:
            tf.extractall(root)  # noqa: S202 - a release tarball of our own pinned project
        os.chmod(binary, os.stat(binary).st_mode | stat.S_IEXEC)
    digest = sha256_file(tarball) if os.path.isfile(tarball) else sha256_file(binary)
    return {"version": version, "source": f"official release asset {asset} (release page publishes no digest; "
            "the asset's own sha256 is recorded)", "binary": binary, "binary_sha256": digest, "asset": asset,
            "stdlib": stdlib}


def unavailable_record(version: str, reason: str) -> dict:
    return {"version": version, "source": "unavailable", "binary": None, "binary_sha256": None, "reason": reason}


def build_source(repo_dir: str, version: str, cargo_home: str, target_dir: str, rustc_toolchain: str) -> dict:
    """Alternative to :func:`fetch_npm_release` for ZoKrates <= 0.7.x: a *native* source build of the
    pinned tag with an already-installed stable ``rustc_toolchain`` and ``RUSTC_BOOTSTRAP=1`` (a
    documented rustc escape hatch letting a stable compiler accept the unstable features it still
    implements internally -- no toolchain is installed by this call).  Confirmed against 0.6.1: this
    crate needs ``#![feature(box_patterns, box_syntax)]`` (nightly-only even at the time; later removed
    from rustc outright), which rustc 1.60.0 still implements and accepts this way.  Avoids the Node/WASM
    runtime of :func:`fetch_npm_release` -- same compiler, run natively; Cargo.lock (already in the
    checkout at the pinned tag) is the build's reproducibility pin, recorded by its own sha256."""
    lock = os.path.join(repo_dir, "Cargo.lock")
    binary = os.path.join(target_dir, "release", "zokrates")
    if not os.path.isfile(binary):
        os.makedirs(cargo_home, exist_ok=True)
        os.makedirs(target_dir, exist_ok=True)
        env = dict(os.environ, CARGO_HOME=cargo_home, CARGO_TARGET_DIR=target_dir, RUSTC_BOOTSTRAP="1")
        r = subprocess.run(["cargo", f"+{rustc_toolchain}", "build", "--release", "--locked", "-p", "zokrates_cli"],
                          cwd=repo_dir, env=env, capture_output=True, text=True, timeout=1800)
        if r.returncode != 0 or not os.path.isfile(binary):
            raise ToolchainError(f"source build failed (rc={r.returncode}): {(r.stderr or r.stdout)[-2000:]}")
    stdlib = os.path.join(repo_dir, "zokrates_stdlib", "stdlib")
    return {"version": version, "source": f"source build of tag {version} ({repo_dir}), rustc {rustc_toolchain} "
            "with RUSTC_BOOTSTRAP=1 (an already-installed stable toolchain; the crate needs "
            "#![feature(box_patterns, box_syntax)], nightly-only and later removed from rustc outright)",
            "binary": binary, "binary_sha256": sha256_file(binary), "cargo_lock_sha256": sha256_file(lock),
            "stdlib": stdlib, "legacy": True}


def run(pin: dict, args: list[str], cwd: str, env_extra: dict | None = None, timeout: float = 120,
       input_text: str | None = None) -> subprocess.CompletedProcess:
    # >= 0.7.x resolves a non-relative import against ZOKRATES_STDLIB; 0.6.1 has no such variable and
    # resolves it against ZOKRATES_HOME instead (set to the stdlib directory itself, not its parent).
    env = {"PATH": "/usr/bin:/bin", "ZOKRATES_STDLIB": pin["stdlib"], "ZOKRATES_HOME": pin["stdlib"], "HOME": cwd}
    if env_extra:
        env.update(env_extra)
    return subprocess.run([pin["binary"]] + args, cwd=cwd, env=env, capture_output=True, text=True,
                          timeout=timeout, input=input_text)


def version_string(pin: dict) -> str:
    r = run(pin, ["--version"], cwd=os.path.dirname(pin["binary"]) or ".", timeout=30)
    return (r.stdout or r.stderr).strip()
