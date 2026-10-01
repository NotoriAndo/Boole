"""halo2 extraction harness: scratch-copy instrumentation, wrapper injection, build and runs.

Each repository is built at its ledger pin in a scratch checkout.  Two kinds of files are added there (never to a
repository's own tree outside scratch):

* the exporter ``halo2_harness/export/<line>.rs`` becomes ``src/dev/boole_export.rs`` of the ``halo2_proofs`` the
  repository builds against (its own path crate, or a copy of the locked registry / git source patched in with
  ``[patch]``); ``dev.rs`` declares it and ``MockProver::assign_advice`` gets one call that records the region and
  annotation of each advice assignment.  The exporter only reads the prover after ``MockProver::run``;
* the wrapper modules ``halo2_harness/wrappers/<adapter>/*.rs`` become ``cfg(test)`` child modules of the chip (or
  of the chip's own test module, so they reuse the repository's test configuration).

Wrappers are ``#[test]`` functions run by the crate's own test binary (``cargo test --release --lib --no-run`` with
the repository's pinned toolchain when installed).  Each writes ``sample_<k>.json`` documents (``boole-halo2-ir/v1``)
and, on request, MockProver verdicts on single-cell mutants (``mutants.out``).
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import time
from dataclasses import dataclass, field

_HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS_DIR = os.path.join(_HERE, "halo2_harness")
EXPORT_DIR = os.path.join(HARNESS_DIR, "export")
WRAPPER_DIR = os.path.join(HARNESS_DIR, "wrappers")
RUSTUP_TOOLCHAINS = os.path.expanduser("~/.rustup/toolchains")


class HarnessError(RuntimeError):
    pass


# ------------------------------------------------------------------------------------------ instrumentation

ZCASH_V03_DECL_ANCHOR = "pub mod metadata;"
ZCASH_V03_SIG_OLD = "fn assign_advice<V, VR, A, AR>(\n        &mut self,\n        _: A,"
ZCASH_V03_SIG_NEW = "fn assign_advice<V, VR, A, AR>(\n        &mut self,\n        boole_annotation: A,"
ZCASH_V03_BODY_OLD = ("            return Err(Error::not_enough_rows_available(self.k));\n        }\n\n"
                      "        if let Some(region) = self.current_region.as_mut() {\n"
                      "            region.update_extent(column.into(), row);\n"
                      "            region.cells.push((column.into(), row));")
ZCASH_V03_HOOK = ("        boole_export::note_advice(\n"
                  "            self.regions.len(),\n"
                  "            &self.current_region.as_ref().map(|r| r.name.clone()).unwrap_or_default(),\n"
                  "            column.index(),\n"
                  "            row,\n"
                  "            boole_annotation().into(),\n"
                  "        );\n")

SCROLL_V1_HOOK_AFTER = "        let advice_anno = anno().into();\n"
SCROLL_V1_HOOK = ("        boole_export::note_advice(\n"
                  "            self.regions.len(),\n"
                  "            &self.current_region.as_ref().map(|r| r.name.clone()).unwrap_or_default(),\n"
                  "            column.index(),\n"
                  "            row,\n"
                  "            String::clone(&advice_anno),\n"
                  "        );\n")

EXPORT_LINES = {"zcash-0.3": "zcash_v03.rs", "scroll-v1": "scroll_v1.rs"}


def instrument_proofs(crate_dir: str, line: str) -> dict:
    """Adds the exporter to the ``halo2_proofs`` crate at ``crate_dir`` (idempotent); returns what was changed."""
    if line not in EXPORT_LINES:
        raise HarnessError(f"no exporter for halo2 line {line!r}")
    dev_rs = os.path.join(crate_dir, "src", "dev.rs")
    with open(dev_rs, encoding="utf-8") as f:
        text = f.read()
    src = os.path.join(EXPORT_DIR, EXPORT_LINES[line])
    shutil.copyfile(src, os.path.join(crate_dir, "src", "dev", "boole_export.rs"))
    if "pub mod boole_export;" not in text:
        if line == "zcash-0.3":
            if text.count(ZCASH_V03_DECL_ANCHOR) != 1 or text.count(ZCASH_V03_SIG_OLD) != 1:
                raise HarnessError("halo2_proofs dev.rs does not have the expected zcash 0.3 shape")
            text = text.replace(ZCASH_V03_DECL_ANCHOR, ZCASH_V03_DECL_ANCHOR + "\npub mod boole_export;", 1)
            sig = text.index(ZCASH_V03_SIG_OLD)
            text = text.replace(ZCASH_V03_SIG_OLD, ZCASH_V03_SIG_NEW, 1)
            body = text.index(ZCASH_V03_BODY_OLD, sig)
            cut = body + ZCASH_V03_BODY_OLD.index("        if let Some(region)")
            text = text[:cut] + ZCASH_V03_HOOK + "\n" + text[cut:]
        elif line == "scroll-v1":
            if text.count(ZCASH_V03_DECL_ANCHOR) != 1 or text.count(SCROLL_V1_HOOK_AFTER) != 1:
                raise HarnessError("halo2_proofs dev.rs does not have the expected scroll v1 shape")
            text = text.replace(ZCASH_V03_DECL_ANCHOR, ZCASH_V03_DECL_ANCHOR + "\npub mod boole_export;", 1)
            text = text.replace(SCROLL_V1_HOOK_AFTER, SCROLL_V1_HOOK_AFTER + SCROLL_V1_HOOK, 1)
        with open(dev_rs, "w", encoding="utf-8") as f:
            f.write(text)
    return {"exporter": EXPORT_LINES[line], "exporter_sha256": _sha(src), "dev_rs": "declares boole_export; "
            "MockProver::assign_advice records (region, annotation) of each advice assignment"}


def _sha(path: str) -> str:
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


# ------------------------------------------------------------------------------------------ wrapper injection

@dataclass
class Injection:
    """One wrapper file: ``source`` (in WRAPPER_DIR) → ``dest`` (relative to the crate), declared in ``parent``
    either at the end of the file (``after`` empty) or right after the first line containing ``after``."""
    source: str
    dest: str
    parent: str
    decl: str
    after: str = ""


PASTA_BLOCK = re.compile(r"// BEGIN pasta.*?// END pasta\n", re.S)


def inject_wrappers(crate_dir: str, injections: list[Injection], driver_field: str = "") -> list[dict]:
    """Copies the wrapper files and declares them.  ``driver_field`` replaces the driver's Pasta block (the
    wrappers' field alias) for adapters over other fields."""
    done = []
    for inj in injections:
        src = os.path.join(WRAPPER_DIR, inj.source)
        dest = os.path.join(crate_dir, inj.dest)
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        shutil.copyfile(src, dest)
        if driver_field and inj.source.endswith("boole_h1.rs"):
            with open(dest, encoding="utf-8") as f:
                text = f.read()
            text, n = PASTA_BLOCK.subn("/// The wrappers' field alias for this adapter.\n" + driver_field + "\n", text)
            if n != 1:
                raise HarnessError("driver Pasta block not found")
            with open(dest, "w", encoding="utf-8") as f:
                f.write(text)
        parent = os.path.join(crate_dir, inj.parent)
        with open(parent, encoding="utf-8") as f:
            text = f.read()
        if inj.decl not in text:
            if inj.after:
                i = text.index(inj.after)
                j = text.index("\n", i + len(inj.after)) + 1
                text = text[:j] + "    " + inj.decl + "\n" + text[j:]
            else:
                text = text.rstrip("\n") + "\n\n" + inj.decl + "\n"
            with open(parent, "w", encoding="utf-8") as f:
                f.write(text)
        done.append({"file": inj.source, "sha256": _sha(src), "dest": inj.dest, "parent": inj.parent})
    return done


# ------------------------------------------------------------------------------------------ vendored crates

def assemble_vendored(crate_dir: str, repo_root: str, cargo_toml_template: str, subst: dict, files: list,
                      lockfile: str, driver_field: str = "") -> list[dict]:
    """A minimal crate in scratch: ``files`` are (source, destination) pairs; a source starting with ``repo:`` is a
    file of the pinned checkout copied verbatim, any other source is a file of WRAPPER_DIR.  The repository's
    Cargo.lock pins the versions of every dependency the crate shares with it."""
    if os.path.isdir(crate_dir):
        shutil.rmtree(crate_dir)
    os.makedirs(os.path.join(crate_dir, "src"))
    with open(os.path.join(WRAPPER_DIR, cargo_toml_template), encoding="utf-8") as f:
        text = f.read()
    for k, v in subst.items():
        text = text.replace("{" + k + "}", v)
    with open(os.path.join(crate_dir, "Cargo.toml"), "w", encoding="utf-8") as f:
        f.write(text)
    if lockfile:
        shutil.copyfile(os.path.join(repo_root, lockfile), os.path.join(crate_dir, "Cargo.lock"))
    done = []
    for src, dst in files:
        path = os.path.join(repo_root, src[5:]) if src.startswith("repo:") else os.path.join(WRAPPER_DIR, src)
        out = os.path.join(crate_dir, dst)
        os.makedirs(os.path.dirname(out), exist_ok=True)
        shutil.copyfile(path, out)
        if driver_field and src.endswith("boole_h1.rs"):
            with open(out, encoding="utf-8") as f:
                text = f.read()
            text, n = PASTA_BLOCK.subn("/// The wrappers' field alias for this adapter.\n" + driver_field + "\n", text)
            if n != 1:
                raise HarnessError("driver Pasta block not found")
            with open(out, "w", encoding="utf-8") as f:
                f.write(text)
        done.append({"file": src, "sha256": _sha(path), "dest": dst})
    return done


# ------------------------------------------------------------------------------------------ registry patch

def patch_registry_crate(workspace: str, crate: str, version: str, tc: "Toolchain", env: dict, dest_root: str,
                         line: str) -> dict:
    """Instrument a crates.io dependency: ``cargo fetch`` (the repository's lockfile), copy the locked source of
    ``crate``-``version`` out of the registry cache, add the exporter and point ``[patch.crates-io]`` of the
    workspace at the copy (same name and version, so the lock keeps every other package)."""
    p = subprocess.run([os.path.join(tc.bin, "cargo"), "fetch"], cwd=workspace, env=cargo_env(env, tc),
                       capture_output=True, text=True, timeout=3600)
    if p.returncode != 0:
        raise HarnessError(f"cargo fetch failed: {p.stderr[-600:]}")
    reg = os.path.join(env["CARGO_HOME"], "registry", "src")
    srcs = [os.path.join(reg, d, f"{crate}-{version}") for d in sorted(os.listdir(reg))
            if os.path.isdir(os.path.join(reg, d, f"{crate}-{version}"))]
    if not srcs:
        raise HarnessError(f"{crate}-{version} not in the registry cache")
    dest = os.path.join(dest_root, f"{crate}-{version}")
    if os.path.isdir(dest):
        shutil.rmtree(dest)
    shutil.copytree(srcs[0], dest)
    info = instrument_proofs(dest, line)
    cargo_toml = os.path.join(workspace, "Cargo.toml")
    with open(cargo_toml, encoding="utf-8") as f:
        text = f.read()
    if "[patch.crates-io]" not in text:
        text = text.rstrip("\n") + f'\n\n[patch.crates-io]\n{crate} = {{ path = "{dest}" }}\n'
        with open(cargo_toml, "w", encoding="utf-8") as f:
            f.write(text)
    info.update(patched=f"{crate} {version} (crates.io source copied from the registry cache)")
    return info


# ------------------------------------------------------------------------------------------ build and run

@dataclass
class Toolchain:
    name: str                      # rustup toolchain directory name
    reason: str

    @property
    def bin(self) -> str:
        return os.path.join(RUSTUP_TOOLCHAINS, self.name, "bin")

    def available(self) -> bool:
        return os.path.exists(os.path.join(self.bin, "cargo"))


def cargo_env(base_env: dict, tc: Toolchain) -> dict:
    env = dict(base_env)
    env["PATH"] = tc.bin + ":" + env.get("PATH", "/usr/bin:/bin")
    env["RUSTC"] = os.path.join(tc.bin, "rustc")
    env["RUSTDOC"] = os.path.join(tc.bin, "rustdoc")
    env.pop("RUSTUP_TOOLCHAIN", None)
    return env


@dataclass
class Built:
    exe: str
    secs: float
    log: str
    lock_diff: list = field(default_factory=list)


def build_tests(workspace: str, package: str, tc: Toolchain, env: dict, log_path: str, timeout: float = 5400,
                features: list[str] | None = None, bin_name: str = "") -> Built:
    """``cargo test --release --lib --no-run -p <package>`` (or ``cargo build --release --bin <bin_name>`` for a
    vendored crate); returns the executable."""
    lock = os.path.join(workspace, "Cargo.lock")
    before = open(lock, encoding="utf-8").read() if os.path.exists(lock) else ""
    if bin_name:
        cmd = [os.path.join(tc.bin, "cargo"), "build", "--release", "--bin", bin_name, "--message-format=json"]
    else:
        cmd = [os.path.join(tc.bin, "cargo"), "test", "--release", "--lib", "--no-run", "-p", package,
               "--message-format=json"]
    if features:
        cmd += ["--features", ",".join(features)]
    t0 = time.time()
    p = subprocess.run(cmd, cwd=workspace, env=cargo_env(env, tc), capture_output=True, text=True, timeout=timeout)
    secs = time.time() - t0
    with open(log_path, "w", encoding="utf-8") as f:
        f.write(p.stderr[-200000:])
    exe = ""
    errors = []
    for ln in p.stdout.splitlines():
        try:
            m = json.loads(ln)
        except ValueError:
            continue
        if m.get("reason") == "compiler-message" and (m.get("message") or {}).get("level") == "error":
            errors.append((m["message"].get("rendered") or "").strip())
        if m.get("reason") == "compiler-artifact" and m.get("executable"):
            tgt = m.get("target", {})
            if bin_name and tgt.get("kind") == ["bin"] and tgt.get("name") == bin_name:
                exe = m["executable"]
            elif not bin_name and tgt.get("kind") == ["lib"] and \
                    tgt.get("name", "").replace("-", "_") == package.replace("-", "_"):
                exe = m["executable"]
    with open(log_path, "a", encoding="utf-8") as f:
        f.write("\n\n".join(errors))
    if p.returncode != 0 or not exe:
        tail = "\n".join(errors)[:1500] or "\n".join(
            ln for ln in p.stderr.splitlines() if ln.startswith(("error", "  -->")))[:1500]
        raise HarnessError(f"cargo test build failed (rc {p.returncode}): {tail or p.stderr[-800:]}")
    after = open(lock, encoding="utf-8").read() if os.path.exists(lock) else ""
    return Built(exe, secs, log_path, _lock_diff(before, after))


def _lock_packages(text: str) -> dict[str, str]:
    out, name, ver, src = {}, None, None, ""
    for ln in text.splitlines() + ["[[package]]"]:
        if ln.startswith("[[package]]"):
            if name:
                out[f"{name} {ver}"] = src
            name, ver, src = None, None, ""
        elif ln.startswith('name = "'):
            name = ln.split('"')[1]
        elif ln.startswith('version = "'):
            ver = ln.split('"')[1]
        elif ln.startswith('source = "'):
            src = ln.split('"')[1]
    return out


def _lock_diff(before: str, after: str) -> list[dict]:
    """Packages the build added or re-sourced in Cargo.lock (a package at a version the lockfile did not pin is a
    deviation; entries the build no longer needs are only counted)."""
    a, b = _lock_packages(before), _lock_packages(after)
    diff = [{"package": k, "before": a.get(k), "after": b[k]} for k in sorted(b) if a.get(k) != b[k]]
    removed = len(set(a) - set(b))
    if removed:
        diff.append({"unused_entries_dropped": removed})
    return diff


def run_target(exe: str, test_name: str, out_dir: str, env: dict, samples: int, seed: int, mutants: bool = False,
               timeout: float = 1800, bin_mode: bool = False) -> dict:
    """Runs one wrapper test; returns the run log (rc, seconds, stderr tail)."""
    e = dict(env)
    e.update(BOOLE_H1_OUT=out_dir, BOOLE_H1_SAMPLES=str(samples), BOOLE_H1_SEED=str(seed),
             BOOLE_H1_MUTANTS="1" if mutants else "0")
    t0 = time.time()
    try:
        args = [exe, test_name] if bin_mode else [exe, test_name, "--exact", "--test-threads=1"]
        p = subprocess.run(args, env=e, capture_output=True, text=True, timeout=timeout)
        rc, err = p.returncode, (p.stdout[-1500:] + p.stderr[-1500:])
    except subprocess.TimeoutExpired:
        rc, err = -9, "timeout"
    return {"rc": rc, "secs": round(time.time() - t0, 2), "tail": err[-1500:]}
