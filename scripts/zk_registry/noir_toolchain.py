"""Pinned Noir compilers, the per-version ACIR tool, nargo invocation and git dependency prefetch.

Compiler selection (per repository; every attempt is recorded):

1. aztec-packages: the ``noir/noir-repo`` submodule commit of the pinned aztec-packages commit;
2. a repository toolchain pin: CI matrix (``noirup`` toolchain), a README ``noirup -v`` instruction, a
   constants file, the noir crates the repository's Rust code pins, the ``noir_version`` of the
   repository's committed artifacts, or the aztec-packages release its Noir dependencies are tagged
   with (that release's ``noir/noir-repo`` source);
3. the noir standard library: the release whose tag is the pinned commit;
4. no pin: the newest noir release published before the pinned commit date whose version satisfies
   ``compiler_version`` (recorded as ``unpinned``).

Binaries are public noir-lang release assets (``noir-aarch64-apple-darwin.tar.gz``) checked against the
release page digest where the page publishes one (otherwise the asset digest is recorded), or a
source build of the exact tag/commit (``cargo build --release --locked -p nargo_cli``).  ``nargo``
runs with ``HOME`` in scratch (its git dependency cache is ``$HOME/nargo``), a restricted ``PATH`` and
no network: every git dependency is fetched beforehand at its tag (:func:`prefetch_git_deps`).
"""
from __future__ import annotations

import json
import os
import re
import subprocess
from dataclasses import dataclass, field

from . import lean_runner as L

SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"


@dataclass(frozen=True)
class Compiler:
    tag: str                       # directory name under the tools root
    version: str                   # `nargo --version` package version
    commit: str                    # noir-lang/noir commit (or aztec-packages commit for its source build)
    source: str                    # how the binary was obtained
    asset_sha256: str = ""         # release asset digest (tarball)
    published_digest: bool = False

    def to_json(self) -> dict:
        return {"tag": self.tag, "version": self.version, "commit": self.commit, "source": self.source,
                "asset_sha256": self.asset_sha256, "published_digest": self.published_digest}


COMPILERS = {c.tag: c for c in [
    Compiler("aztec-v0.67.0", "1.0.0-beta.0", "19a500ffc09ab8bc367a78599dd73a07a04b426e",
             "source build of aztec-packages tag aztec-packages-v0.67.0, directory noir/noir-repo "
             "(cargo build --release --locked -p nargo_cli, rustc 1.85.0)"),
    Compiler("v1.0.0-beta.0", "1.0.0-beta.0", "7311d8ca566c3b3e0744389fc5e4163741927767",
             "noir-lang/noir release v1.0.0-beta.0 asset nargo-aarch64-apple-darwin.tar.gz (no published digest)",
             "83a32e1ade0f7f0626990bc6ae64f40979231e48cb10589fd8b17e4739c5df64", False),
    Compiler("v1.0.0-beta.5", "1.0.0-beta.5", "c651df6e2bf5db3966aa0c95abea2fc4c69d4513",
             "noir-lang/noir release v1.0.0-beta.5 asset noir-aarch64-apple-darwin.tar.gz (no published digest)",
             "04836880c47e81c3b85f68b9e26b96472c211005679de576de2e3163d8e39579", False),
    Compiler("v1.0.0-beta.14", "1.0.0-beta.14", "60ccd48e18ad8ce50d5ecda9baf813b712145051",
             "noir-lang/noir release v1.0.0-beta.14 asset noir-aarch64-apple-darwin.tar.gz (published digest)",
             "d0e80cc795c48cf887793d1a0edb41899c8232729b1f5ba1b76c0f3f45575268", True),
    Compiler("v1.0.0-beta.16", "1.0.0-beta.16", "2d46fca7203545cbbfb31a0d0328de6c10a8db95",
             "noir-lang/noir release v1.0.0-beta.16 asset noir-aarch64-apple-darwin.tar.gz (published digest)",
             "90c53428c8ab6585a7e57eb5e27d3689939a76c63a4d6b1ba448d865fd394199", True),
    Compiler("v1.0.0-beta.25", "1.0.0-beta.25", "75061fab15986eedee4e7d9104ff87dd9fa4ca10",
             "noir-lang/noir release v1.0.0-beta.25 asset noir-aarch64-apple-darwin.tar.gz (published digest)",
             "5ddb45016c8e100d7e03d10e106803111c71a07a920441006cb4816d4a94f624", True),
    Compiler("v1.0.0-beta.26", "1.0.0-beta.26", "40d6574f851d926f93e0c3a271bac3e6e82ac905",
             "noir-lang/noir release v1.0.0-beta.26 asset noir-aarch64-apple-darwin.tar.gz (published digest)",
             "a8d01f4f554358dfdb200ec433fc64231d9c5614e30e65f4d27cca18a0cf24e5", True),
    Compiler("v1.0.0-rc.2", "1.0.0-rc.2", "0ecc97a242ed37c0d1567e25747ed8d4c59cae49",
             "noir-lang/noir release v1.0.0-rc.2 asset noir-aarch64-apple-darwin.tar.gz (published digest)",
             "e44a46a3c35e89b064948c054562da36b8cc58bfb31623ff27e5440c3a0bb2bc", True),
]}

# Repository -> (compiler attempts in order, how the first one is pinned).
REPO_COMPILERS = {
    "AztecProtocol/aztec-packages": (["v1.0.0-beta.25"],
                                     "noir/noir-repo submodule gitlink 75061fab at the pinned commit = tag v1.0.0-beta.25"),
    "noir-lang/noir": (["v1.0.0-rc.2"], "the pinned commit 0ecc97a2 is the release tag v1.0.0-rc.2 (stdlib embedded in nargo)"),
    "polybase/payy": (["v1.0.0-beta.14"],
                      "noir/README.md `noirup -v 1.0.0-beta.14` and the noir crates of Cargo.toml (tag v1.0.0-beta.14)"),
    "zkemail/zkemail.nr": (["v1.0.0-beta.5"], ".github/workflows/test.yml noirup toolchain matrix [1.0.0-beta.5]"),
    "keep-starknet-strange/garaga": (["v1.0.0-beta.16"], "tools/make/constants.json noir.nargo_version 1.0.0-beta.16"),
    "mach-34/z-imburse": (["aztec-v0.67.0"],
                          "Noir dependencies tagged aztec-packages-v0.67.0 (the nargo of that release's noir/noir-repo; "
                          "the committed artifacts record noir_version 1.0.0-beta.0). The noir release v1.0.0-beta.0 "
                          "was tried in the pilot and does not compile aztec-packages-v0.67.0's aztec-nr "
                          "(BoundedVec::from_parts_unchecked missing), so it is not an attempt"),
    "selfxyz/self": (["v1.0.0-beta.26"],
                     "unpinned (compiler_version >=0.36.0, no CI pin): newest noir release before the pinned commit "
                     "date 2026-09-06 (v1.0.0-beta.26, 2026-07-30)"),
}


@dataclass
class Toolchain:
    tools_root: str                # <tools_root>/<tag>/nargo and boole-acir-tool
    home: str                      # HOME for nargo (git dependency cache)
    scratch: str

    def nargo(self, tag: str) -> str:
        return os.path.join(self.tools_root, tag, "nargo")

    def acir_tool(self, tag: str) -> str:
        return os.path.join(self.tools_root, tag, "boole-acir-tool")

    def env(self) -> dict:
        tmp = os.path.join(self.scratch, "tmp")
        os.makedirs(tmp, exist_ok=True)
        return {"PATH": SYSTEM_PATH, "HOME": self.home, "TMPDIR": tmp, "LANG": "C.UTF-8",
                "PYTHONDONTWRITEBYTECODE": "1", "GIT_TERMINAL_PROMPT": "0", "GIT_CONFIG_GLOBAL": "/dev/null",
                "GIT_CONFIG_NOSYSTEM": "1", "NO_COLOR": "1", "RUST_BACKTRACE": "0",
                # nargo clones missing git dependencies itself: refuse every network protocol, so a compile
                # only ever sees the dependencies fetched beforehand at their pins
                "GIT_ALLOW_PROTOCOL": "file"}

    def run_nargo(self, tag: str, args: list[str], cwd: str, timeout: float, rss_limit_mb: int | None = 12288,
                  watch=None) -> L.RunResult:
        return L.run_process([self.nargo(tag)] + args, self.env(), cwd, timeout, rss_limit_mb, watch=watch)

    def run_tool(self, tag: str, args: list[str], cwd: str, timeout: float = 600) -> L.RunResult:
        return L.run_process([self.acir_tool(tag)] + args, self.env(), cwd, timeout, 8192)

    def binary_info(self, tag: str) -> dict:
        from .package import sha256_file
        info = COMPILERS[tag].to_json()
        info["nargo_sha256"] = sha256_file(self.nargo(tag))
        info["acir_tool_sha256"] = sha256_file(self.acir_tool(tag))
        return info


# ------------------------------------------------------------------------------------------ Nargo.toml

def read_toml(path: str) -> dict:
    """Minimal TOML reader for Nargo.toml: tables, ``key = value`` with strings, numbers, arrays and inline
    tables (enough for [package] / [dependencies] / [workspace])."""
    out: dict = {}
    cur = out
    with open(path, encoding="utf-8") as f:
        text = f.read()
    for raw in text.splitlines():
        line = re.sub(r"(?<![\"'])#.*$", "", raw).strip()
        if not line:
            continue
        m = re.match(r"\[([^\]]+)\]$", line)
        if m:
            cur = out
            for part in m.group(1).split("."):
                cur = cur.setdefault(part.strip(), {})
            continue
        m = re.match(r"([A-Za-z0-9_\-\"]+)\s*=\s*(.+)$", line)
        if m:
            cur[m.group(1).strip('"')] = _toml_value(m.group(2).strip())
    return out


def _toml_value(s: str):
    s = s.strip()
    if s.startswith("{") and s.endswith("}"):
        d = {}
        for part in _split_commas(s[1:-1]):
            if "=" in part:
                k, v = part.split("=", 1)
                d[k.strip().strip('"')] = _toml_value(v)
        return d
    if s.startswith("[") and s.endswith("]"):
        return [_toml_value(x) for x in _split_commas(s[1:-1]) if x.strip()]
    if len(s) >= 2 and s[0] == s[-1] and s[0] in "\"'":
        return s[1:-1]
    try:
        return int(s)
    except ValueError:
        return s


def _split_commas(s: str) -> list[str]:
    parts, depth, cur, inq = [], 0, [], ""
    for c in s:
        if c in "\"'" and (not inq or inq == c):
            inq = "" if inq else c
        if not inq and c in "{[":
            depth += 1
        elif not inq and c in "}]":
            depth -= 1
        if c == "," and depth == 0 and not inq:
            parts.append("".join(cur).strip())
            cur = []
        else:
            cur.append(c)
    if "".join(cur).strip():
        parts.append("".join(cur).strip())
    return parts


def git_dep_dir(home: str, url: str, tag: str) -> str:
    """nargo's download location ``$HOME/nargo/<host>/<url path>/<tag>``."""
    m = re.match(r"https?://([^/]+)(/.*)?$", url)
    host, path = m.group(1), (m.group(2) or "")
    return os.path.join(home, "nargo", host, path.strip("/"), tag)


@dataclass
class DepRecord:
    name: str
    url: str
    tag: str
    directory: str
    path: str
    commit: str = ""
    resolved_by: str = ""

    def to_json(self) -> dict:
        return {"name": self.name, "version": self.tag, "source": self.url, "path": self.path,
                "resolved_by": self.resolved_by or f"git tag {self.tag} @ {self.commit[:12]}"}


def _git(args: list[str], cwd: str, timeout: float = 900) -> subprocess.CompletedProcess:
    env = {"PATH": SYSTEM_PATH, "HOME": cwd, "GIT_TERMINAL_PROMPT": "0", "GIT_CONFIG_GLOBAL": "/dev/null",
           "GIT_CONFIG_NOSYSTEM": "1"}
    return subprocess.run(["git"] + args, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)


def fetch_tag(dest: str, url: str, tag: str, sparse: list[str] | None = None) -> str:
    """Shallow fetch of ``tag`` (a tag or branch name) into ``dest``; returns the commit."""
    os.makedirs(dest, exist_ok=True)
    if os.path.exists(os.path.join(dest, ".git", ".boole-fetch-done")):
        return _git(["rev-parse", "HEAD"], dest).stdout.strip()
    _git(["init", "-q"], dest)
    args = ["fetch", "-q", "--depth", "1"] + (["--filter=blob:none"] if sparse else []) + [url]
    r = _git(args + [f"refs/tags/{tag}"], dest)
    if r.returncode != 0:
        r = _git(args + [tag], dest)
        if r.returncode != 0:
            raise RuntimeError(f"git fetch {url} {tag}: {r.stderr[-300:]}")
    if sparse:
        _git(["sparse-checkout", "set", "--no-cone"] + sparse, dest)
    r = _git(["-c", "advice.detachedHead=false", "checkout", "-q", "FETCH_HEAD"], dest)
    if r.returncode != 0:
        raise RuntimeError(f"git checkout {url} {tag}: {r.stderr[-300:]}")
    open(os.path.join(dest, ".git", ".boole-fetch-done"), "w").close()
    return _git(["rev-parse", "HEAD"], dest).stdout.strip()


def sparse_add(dest: str, patterns: list[str]) -> None:
    cur = _git(["sparse-checkout", "list"], dest).stdout.split()
    new = [p for p in patterns if p not in cur]
    if new:
        _git(["sparse-checkout", "set", "--no-cone"] + cur + new, dest)


LARGE_REPOS = ("aztec-packages",)


def _concat_alias(home: str, url: str, tag: str, dest: str) -> None:
    """The v1.0.0-beta.0 release of nargo names its download directory ``<url path><tag>`` (no separator);
    a symlink to the same checkout serves it."""
    alias = dest.rstrip("/")
    alias = os.path.join(os.path.dirname(os.path.dirname(alias)), os.path.basename(os.path.dirname(alias)) + tag)
    if not os.path.lexists(alias):
        try:
            os.symlink(dest, alias)
        except FileExistsError:
            pass


def prefetch_git_deps(crate_dir: str, home: str, seen: dict | None = None, pins: dict | None = None) -> dict:
    """Fetch every git dependency reachable from ``crate_dir`` (path dependencies followed) into nargo's
    cache under ``home``; returns {(url, tag, directory): DepRecord}.  ``pins`` maps (url, tag) to a commit
    for moving branch names (e.g. ``tag = "master"``), resolved at the repository pin date."""
    seen = {} if seen is None else seen
    manifest = os.path.join(crate_dir, "Nargo.toml")
    if not os.path.exists(manifest):
        return seen
    deps = read_toml(manifest).get("dependencies", {})
    for name, spec in deps.items():
        if not isinstance(spec, dict):
            continue
        if "path" in spec:
            sub = os.path.normpath(os.path.join(crate_dir, spec["path"]))
            key = ("path", sub, "")
            if key not in seen:
                seen[key] = None
                prefetch_git_deps(sub, home, seen, pins)
            continue
        if "git" not in spec:
            continue
        url, tag, directory = spec["git"], spec.get("tag", ""), spec.get("directory", "").strip("./")
        key = (url, tag, directory)
        if key in seen:
            continue
        dest = git_dep_dir(home, url, tag)
        large = any(x in url.lower() for x in LARGE_REPOS)
        sparse = [f"/{directory}/"] if (large and directory) else None
        if pins and (url, tag) in pins:
            commit = fetch_tag(dest, url, pins[(url, tag)][0], sparse)
            rec = DepRecord(name, url, tag, directory, dest, commit, pins[(url, tag)][1])
        else:
            commit = fetch_tag(dest, url, tag, sparse)
            rec = DepRecord(name, url, tag, directory, dest, commit)
        if sparse:
            sparse_add(dest, sparse)
        seen[key] = rec
        _concat_alias(home, url, tag, dest)
        pkg = os.path.join(dest, directory) if directory else dest
        if large:
            _follow_sparse_paths(dest, pkg)
        prefetch_git_deps(pkg, home, seen, pins)
    return seen


def _follow_sparse_paths(repo_root: str, pkg: str, done: set | None = None) -> None:
    """Add the path dependencies of a sparse checkout package to the sparse set (recursively)."""
    done = set() if done is None else done
    if pkg in done:
        return
    done.add(pkg)
    manifest = os.path.join(pkg, "Nargo.toml")
    if not os.path.exists(manifest):
        return
    for spec in read_toml(manifest).get("dependencies", {}).values():
        if isinstance(spec, dict) and "path" in spec:
            sub = os.path.normpath(os.path.join(pkg, spec["path"]))
            rel = os.path.relpath(sub, repo_root)
            if not rel.startswith(".."):
                sparse_add(repo_root, [f"/{rel}/"])
                _follow_sparse_paths(repo_root, sub, done)


def crate_of(path: str, root: str) -> str | None:
    """Nearest directory above ``path`` (inside ``root``) whose Nargo.toml has a [package] table."""
    d = os.path.dirname(path)
    while d.startswith(root):
        m = os.path.join(d, "Nargo.toml")
        if os.path.exists(m) and "package" in read_toml(m):
            return d
        if d == root:
            break
        d = os.path.dirname(d)
    return None


def crate_info(crate_dir: str) -> dict:
    t = read_toml(os.path.join(crate_dir, "Nargo.toml"))
    pkg = t.get("package", {})
    return {"name": pkg.get("name", os.path.basename(crate_dir)), "type": pkg.get("type", "bin"),
            "compiler_version": pkg.get("compiler_version", ""), "dependencies": t.get("dependencies", {})}


def load_json(path: str):
    with open(path, encoding="utf-8") as f:
        return json.load(f)
