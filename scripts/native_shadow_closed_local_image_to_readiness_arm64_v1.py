#!/usr/bin/env python3
"""Build the successor guest as a reversible closed-local development image.

This path deliberately does not consume, emulate, or mint the historical A7
production authority.  It verifies the already-sealed P4 -> R3 -> F7 input
chain, then reuses the same image backend and readback verifier as the locked
production implementation.  Its outputs are disposable engineering artifacts:
they establish neither a boot, a release, nor any activation right.
"""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import fcntl
import importlib
import json
import os
import pathlib
import re
import struct
import sys
from collections.abc import Mapping
from typing import Any, Callable, Optional

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts import native_shadow_rootfs_builder_boot_arm64_v4 as builder_v4
from scripts import native_shadow_successor_produce_phase_arm64_v5 as sealed


SCHEMA = "boole.native-shadow.closed-local-image-to-readiness.arm64.v1"
PREFLIGHT_STATUS = "READY-NO-IMAGE-CREATED"
BUILD_STATUS = "CLOSED-LOCAL-IMAGE-VERIFIED"
ARTIFACT_CLASS = "DISPOSABLE-DEVELOPMENT"
RUN_LABEL_PATTERN = r"[a-z0-9][a-z0-9._-]{0,79}"
LOOP_SET_STATUS64 = 0x4C04
LOOP_GET_STATUS64 = 0x4C05
LO_FLAGS_AUTOCLEAR = 4
LOOP_INFO64_SIZE = 232
LOOP_FLAGS_OFFSET = 52
AUTHORITY_STAGING_PATH = "usr/share/boole/native-shadow"
AUTHORITY_MOUNTED_PATH = "/" + AUTHORITY_STAGING_PATH
AUTHORITY_DIRECTORY_MODE = 0o555


class ClosedLocalImageError(RuntimeError):
    pass


def _require_authority_directory(row: Any) -> None:
    if (
        not isinstance(row, Mapping)
        or row.get("kind") != "directory"
        or row.get("mode") != AUTHORITY_DIRECTORY_MODE
        or row.get("uid") != 0
        or row.get("gid") != 0
    ):
        raise ClosedLocalImageError(
            "installed authority directory must be root:root mode 0555"
        )


@contextlib.contextmanager
def _development_authority_directory_contract():
    """Correct and verify the one derived parent that is security authority.

    The sealed source lock tracks the files beneath this directory but does not
    carry a row for the directory itself.  The inherited assembler therefore
    derives it with the generic 0755 parent mode.  The installed-authority
    reader deliberately requires 0555.  Scope the correction to this reversible
    development lane so historical sealed producers remain byte-preserved.
    """

    namespace = builder_v4.materialize_staging_tree.__globals__.get("_IMPL")
    if not isinstance(namespace, dict):
        raise ClosedLocalImageError("development builder namespace is unavailable")
    original = namespace.get("_ensure_parents")
    if not callable(original):
        raise ClosedLocalImageError("development parent derivation is unavailable")

    def ensure_parents(entries):
        original(entries)
        row = entries.get(AUTHORITY_STAGING_PATH)
        if (
            isinstance(row, Mapping)
            and row.get("kind") == "directory"
            and row.get("mode") == 0o755
            and row.get("uid") == 0
            and row.get("gid") == 0
        ):
            row = dict(row, mode=AUTHORITY_DIRECTORY_MODE)
            entries[AUTHORITY_STAGING_PATH] = row
        _require_authority_directory(row)

    namespace["_ensure_parents"] = ensure_parents
    try:
        yield
    finally:
        namespace["_ensure_parents"] = original


@dataclasses.dataclass(frozen=True)
class DevelopmentGenerationChain:
    correction: Mapping[str, Any]
    fresh_rehearsal: Mapping[str, Any]
    fingerprint: Mapping[str, Any]
    identities: Mapping[str, sealed.FileIdentity]
    import_identities: tuple[sealed.FileIdentity, ...]
    output_names: tuple[str, ...] = sealed.OUTPUT_NAMES


def _authorisations() -> dict[str, Any]:
    return {
        **dict(sealed.ZERO_AUTHORISATIONS),
        "activationAllowed": False,
        "imageProductionAuthorised": False,
    }


def _claims() -> dict[str, bool]:
    return {
        "activation": False,
        "bootVerified": False,
        "consensus": False,
        "mining": False,
        "p2p": False,
        "productionRelease": False,
        "reward": False,
        "serving": False,
        "testnet": False,
    }


def _identity_rows(identities) -> list[dict[str, Any]]:
    return [identity.record() for identity in identities]


def verify_development_generation_chain(
    repository_root: pathlib.Path,
) -> DevelopmentGenerationChain:
    """Verify P4 -> R3 -> F7 while requiring A7 and result-v7 to be absent.

    P4/R3/F7 remain historical facts about the exact low-level backend.  A7 is
    a production-only one-shot and is intentionally neither required nor
    simulated by this reversible development path.
    """

    root = pathlib.Path(repository_root).resolve()
    preregistered = sealed.verify_preregistered_generation(root)
    sealed._require_absent(root, sealed.A7_PATH, "historical A7 production authority")
    sealed._require_absent(root, sealed.RESULT_V7_PATH, "historical result-v7")

    p4_id = preregistered.correction_identity
    p4 = preregistered.correction
    generation = _identity_rows(preregistered.generation_identities)
    reused = _identity_rows(preregistered.reused_identities)
    dispatch_fence = p4_id.record()

    r3_id, r3 = sealed._load_canonical(root, sealed.R3_PATH)
    r3_keys = {
        "activationAllowed",
        "authorisations",
        "bootableClaim",
        "boundInputs",
        "effects",
        "executionEnvelope",
        "generationFiles",
        sealed.P4_FIELD,
        "measurement",
        "predecessors",
        "repeatable",
        "reusedPinnedUpstream",
        "schema",
        "status",
    }
    sealed._require_exact_keys(r3, r3_keys, "development R3")
    if r3["schema"] != sealed.R3_SCHEMA or r3["status"] != sealed.R3_STATUS:
        raise ClosedLocalImageError("R3 schema or status differs")
    if r3["activationAllowed"] is not False or r3["bootableClaim"] is not False:
        raise ClosedLocalImageError("R3 claims activation or boot")
    sealed._require_strict_equal(
        r3["authorisations"], sealed.ZERO_AUTHORISATIONS, "R3 authority"
    )
    sealed._require_strict_equal(r3["effects"], sealed.ZERO_EFFECTS, "R3 effects")
    sealed._require_strict_equal(
        r3[sealed.P4_FIELD], dispatch_fence, "R3 P4 identity"
    )
    if r3["predecessors"] != [p4_id.record()]:
        raise ClosedLocalImageError("R3 predecessors differ")
    if r3["generationFiles"] != generation:
        raise ClosedLocalImageError("R3 generation files differ")
    if r3["reusedPinnedUpstream"] != reused:
        raise ClosedLocalImageError("R3 reused inputs differ")
    if r3["boundInputs"] != [p4_id.record(), *generation, *reused]:
        raise ClosedLocalImageError("R3 bound-input union differs")
    if r3["repeatable"] is not True or not isinstance(r3["measurement"], dict):
        raise ClosedLocalImageError("R3 repeatability evidence differs")

    p1_identity, p1 = sealed._load_canonical(root, sealed.P1_PATH)
    if (
        p1_identity.sha256 != sealed.P1_SHA256
        or p1_identity.size_bytes != sealed.P1_SIZE_BYTES
    ):
        raise ClosedLocalImageError("P1 identity differs")
    try:
        expected_measurement = p1["expectedPreflight"]["measurement"]
    except (KeyError, TypeError) as exc:
        raise ClosedLocalImageError("P1 has no sealed staging measurement") from exc
    sealed._require_strict_equal(
        r3["measurement"], expected_measurement, "R3 staging measurement"
    )

    f7_id, f7 = sealed._load_canonical(root, sealed.F7_PATH)
    f7_keys = {
        "authorisations",
        "boundaries",
        "files",
        sealed.P4_FIELD,
        "predecessors",
        "rehearsalGate",
        "schema",
        "status",
        "subject",
        "whatThisRecordDoesNotEstablish",
    }
    sealed._require_exact_keys(f7, f7_keys, "development F7")
    if f7["schema"] != sealed.F7_SCHEMA or f7["status"] != sealed.F7_STATUS:
        raise ClosedLocalImageError("F7 schema or status differs")
    sealed._require_strict_equal(
        f7["authorisations"], sealed.ZERO_AUTHORISATIONS, "F7 authority"
    )
    sealed._require_strict_equal(
        f7["boundaries"],
        {
            "activationAllowed": False,
            "bootableClaim": False,
            "servingClaim": False,
        },
        "F7 boundaries",
    )
    sealed._require_strict_equal(
        f7[sealed.P4_FIELD], dispatch_fence, "F7 P4 identity"
    )
    if f7["files"] != generation:
        raise ClosedLocalImageError("F7 generation files differ")
    if f7["predecessors"] != [p4_id.record(), r3_id.record()]:
        raise ClosedLocalImageError("F7 predecessors differ")
    gate_identity = sealed._read_regular(root, sealed.R3_GATE_PATH)[0]
    if f7["rehearsalGate"] != gate_identity.record():
        raise ClosedLocalImageError("F7 rehearsal gate differs")

    return DevelopmentGenerationChain(
        correction=p4,
        fresh_rehearsal=r3,
        fingerprint=f7,
        identities={"P4": p4_id, "R3": r3_id, "F7": f7_id},
        import_identities=preregistered.import_identities,
    )


def _real_empty_directory(path: pathlib.Path, context: str) -> pathlib.Path:
    try:
        resolved = pathlib.Path(path).resolve(strict=True)
    except OSError as exc:
        raise ClosedLocalImageError(f"{context} is not a real directory") from exc
    if not resolved.is_dir() or pathlib.Path(path).is_symlink():
        raise ClosedLocalImageError(f"{context} is not a real directory")
    if any(resolved.iterdir()):
        raise ClosedLocalImageError(f"{context} is not empty")
    return resolved


def _launcher_bytes(path: pathlib.Path, *, require_sealed: bool) -> bytes:
    if not require_sealed:
        return b"fake-launcher"
    try:
        raw = pathlib.Path(path).read_bytes()
    except OSError as exc:
        raise ClosedLocalImageError("launcher is unreadable") from exc
    if not raw:
        raise ClosedLocalImageError("launcher is empty")
    return raw


def _request(
    *,
    chain: DevelopmentGenerationChain,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    outputs: pathlib.Path,
    scratch: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    backend: Optional[sealed.ImageBackend],
) -> sealed.ProductionRequest:
    return sealed.ProductionRequest(
        repository_root=pathlib.Path(repository_root).resolve(),
        artifact_store=artifact_store,
        outputs=outputs,
        scratch=scratch,
        gpgv=pathlib.Path(gpgv),
        zstd=pathlib.Path(zstd),
        launcher=pathlib.Path(launcher),
        launcher_binary=_launcher_bytes(launcher, require_sealed=backend is None),
        chain=chain,
    )


def _set_loop_autoclear(
    device: str,
    *,
    opener: Callable[[str, int], int] = os.open,
    closer: Callable[[int], None] = os.close,
    ioctl: Callable[..., Any] = fcntl.ioctl,
) -> None:
    """Set the kernel autoclear flag without relying on a nonexistent CLI flag."""

    if re.fullmatch(r"/dev/loop[0-9]+", device) is None:
        raise ClosedLocalImageError("loop device name is unsafe")
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = opener(device, flags)
    try:
        info = bytearray(LOOP_INFO64_SIZE)
        ioctl(descriptor, LOOP_GET_STATUS64, info, True)
        current = struct.unpack_from("=I", info, LOOP_FLAGS_OFFSET)[0]
        struct.pack_into(
            "=I", info, LOOP_FLAGS_OFFSET, current | LO_FLAGS_AUTOCLEAR
        )
        ioctl(descriptor, LOOP_SET_STATUS64, bytes(info))
    finally:
        closer(descriptor)


class DevelopmentAutoclearReadbackEffects:
    """Use the portable loop ioctl and retain explicit normal-path cleanup."""

    def __init__(
        self,
        readback_module: Any,
        *,
        autoclear_setter: Callable[[str], None] = _set_loop_autoclear,
    ) -> None:
        self._readback_module = readback_module
        self._delegate = readback_module.HostReadbackEffects()
        self._set_autoclear = autoclear_setter
        self._autoclear_devices: set[str] = set()

    def unmet_requirements(self) -> list[str]:
        return list(self._delegate.unmet_requirements())

    def setup_loop(self, image: Any) -> str:
        device = self._delegate.setup_loop(image)
        try:
            self._set_autoclear(device)
        except BaseException:
            self._delegate.detach_loop(device)
            raise
        self._autoclear_devices.add(device)
        return device

    def mount(self, device: str, mountpoint: pathlib.Path) -> None:
        self._delegate.mount(device, mountpoint)

    def read_tree(self, mountpoint: pathlib.Path) -> dict[str, dict[str, Any]]:
        tree = dict(self._delegate.read_tree(mountpoint))
        _require_authority_directory(tree.get(AUTHORITY_MOUNTED_PATH))
        return tree

    def unmount(self, mountpoint: pathlib.Path) -> None:
        self._delegate.unmount(mountpoint)

    def detach_loop(self, device: str) -> None:
        try:
            self._delegate.detach_loop(device)
        except BaseException as exc:
            error_type = getattr(self._readback_module, "ReadbackV3Error", None)
            expected = (
                "failed: losetup: %s: detach failed: No such device or address"
                % device
            )
            already_autocleared = (
                device in self._autoclear_devices
                and isinstance(error_type, type)
                and isinstance(exc, error_type)
                and str(exc).endswith(expected)
            )
            if not already_autocleared:
                raise
        finally:
            self._autoclear_devices.discard(device)


class DevelopmentRepositoryImageBackend(sealed.RepositoryImageBackend):
    """Scope the runner-compatible readback adapter to this reversible lane."""

    def prepare(self, request):
        with _development_authority_directory_contract():
            return super().prepare(request)

    def readback(self, repository_root, outputs, chain):
        historical = sealed.AutoclearReadbackEffects
        sealed.AutoclearReadbackEffects = DevelopmentAutoclearReadbackEffects
        try:
            return super().readback(repository_root, outputs, chain)
        finally:
            sealed.AutoclearReadbackEffects = historical


def _development_backend() -> sealed.RepositoryImageBackend:
    # The production loader correctly refuses every repository module that is
    # not named by the historical F7 fingerprint.  This new orchestrator is
    # intentionally outside that old production fingerprint, so the
    # reversible lane uses the backend's injected-loader seam.  The workflow
    # supplies a root-owned, non-writable checkout, while
    # verify_development_generation_chain hashes the complete bound import
    # closure before this loader is reached.
    return DevelopmentRepositoryImageBackend(module_loader=importlib.import_module)


def preflight(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    outputs: pathlib.Path,
    scratch: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    backend: Optional[sealed.ImageBackend] = None,
) -> dict[str, Any]:
    chain = verify_development_generation_chain(repository_root)
    store = pathlib.Path(artifact_store).resolve(strict=True)
    if not store.is_dir():
        raise ClosedLocalImageError("artifact store is not a directory")
    scratch_root = _real_empty_directory(scratch, "preflight scratch")
    if os.path.lexists(outputs):
        raise ClosedLocalImageError("preflight output path already exists")
    request = _request(
        chain=chain,
        repository_root=repository_root,
        artifact_store=store,
        outputs=pathlib.Path(outputs),
        scratch=scratch_root,
        gpgv=gpgv,
        zstd=zstd,
        launcher=launcher,
        backend=backend,
    )
    selected = _development_backend() if backend is None else backend
    prepared = selected.prepare(request)
    if not isinstance(prepared, sealed.PreparedProduction):
        raise ClosedLocalImageError("image backend returned no prepared staging")
    if os.path.lexists(outputs):
        raise ClosedLocalImageError("preflight created an output path")
    return {
        "artifactClass": ARTIFACT_CLASS,
        "authorisations": _authorisations(),
        "claims": _claims(),
        "effects": {"imagesCreated": 0, "machinesStarted": 0},
        "measurement": dict(prepared.measurement),
        "schema": SCHEMA,
        "status": PREFLIGHT_STATUS,
    }


def _output_manifest(outputs: pathlib.Path) -> list[dict[str, Any]]:
    rows = []
    for name in sealed.OUTPUT_NAMES:
        identity, _ = sealed._read_regular(outputs, name)
        rows.append(identity.record())
    return rows


def _publish_result(path: pathlib.Path, document: Mapping[str, Any]) -> None:
    raw = sealed.canonical_json(document)
    target = pathlib.Path(path)
    if os.path.lexists(target):
        raise ClosedLocalImageError("closed-local result already exists")
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_name(target.name + ".partial")
    if os.path.lexists(temporary):
        raise ClosedLocalImageError("closed-local result partial already exists")
    try:
        with temporary.open("xb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, target)
    except BaseException:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
        raise


def build(
    *,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    outputs: pathlib.Path,
    scratch: pathlib.Path,
    result: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    launcher: pathlib.Path,
    run_label: str,
    backend: Optional[sealed.ImageBackend] = None,
) -> dict[str, Any]:
    if re.fullmatch(RUN_LABEL_PATTERN, run_label) is None:
        raise ClosedLocalImageError("development run label is invalid")
    chain = verify_development_generation_chain(repository_root)
    if backend is None and (os.geteuid() != 0 or os.getegid() != 0):
        raise ClosedLocalImageError("real image backend requires root:root")
    store = pathlib.Path(artifact_store).resolve(strict=True)
    if not store.is_dir():
        raise ClosedLocalImageError("artifact store is not a directory")
    scratch_root = _real_empty_directory(scratch, "build scratch")
    output_root = pathlib.Path(outputs)
    if os.path.lexists(output_root):
        raise ClosedLocalImageError("fresh closed-local output path already exists")
    request = _request(
        chain=chain,
        repository_root=repository_root,
        artifact_store=store,
        outputs=output_root,
        scratch=scratch_root,
        gpgv=gpgv,
        zstd=zstd,
        launcher=launcher,
        backend=backend,
    )
    selected = _development_backend() if backend is None else backend
    prepared = selected.prepare(request)
    if not isinstance(prepared, sealed.PreparedProduction):
        raise ClosedLocalImageError("image backend returned no prepared staging")
    output_root.mkdir(mode=0o700)
    kernel = selected.extract_kernel(request, prepared)
    initrd = selected.build_initrd(request, prepared)
    if not isinstance(initrd, bytes) or not initrd:
        raise ClosedLocalImageError("initrd builder returned no bytes")
    sealed._publish_bytes_once(output_root / "guest-initrd", initrd)
    root_disk = selected.build_root_disk(request, prepared)
    verification = selected.verify_images(
        request, prepared, kernel, initrd, root_disk
    )
    if not isinstance(verification, Mapping) or verification.get("passed") is not True:
        raise ClosedLocalImageError("produced image verification did not pass")
    sealed._sync_produced_images(output_root)
    readback = selected.readback(repository_root, output_root, chain)
    if (
        not isinstance(readback, Mapping)
        or readback.get("status") != sealed.READBACK_PASS_STATUS
        or readback.get("mayEnterQualification") is not True
        or readback.get("qualifiedForReplicaComparison") is not True
    ):
        raise ClosedLocalImageError("root-disk readback did not pass")
    manifest = _output_manifest(output_root)
    document = {
        "artifactClass": ARTIFACT_CLASS,
        "authorisations": _authorisations(),
        "buildReceipt": dict(prepared.build_receipt),
        "claims": _claims(),
        "effects": {"imagesCreated": 1, "machinesStarted": 0},
        "generation": {
            name: chain.identities[name].record() for name in ("P4", "R3", "F7")
        },
        "manifest": manifest,
        "measurement": dict(prepared.measurement),
        "readback": dict(readback),
        "runLabel": run_label,
        "schema": SCHEMA,
        "status": BUILD_STATUS,
        "verification": dict(verification),
    }
    _publish_result(result, document)
    return document


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    child = parser.add_subparsers(dest="mode", required=True)
    for mode in ("preflight", "build"):
        command = child.add_parser(mode)
        command.add_argument(
            "--repository-root", type=pathlib.Path, default=pathlib.Path.cwd()
        )
        command.add_argument("--cas", type=pathlib.Path, required=True)
        command.add_argument("--outputs", type=pathlib.Path, required=True)
        command.add_argument("--scratch", type=pathlib.Path, required=True)
        command.add_argument("--gpgv", type=pathlib.Path, required=True)
        command.add_argument("--zstd", type=pathlib.Path, required=True)
        command.add_argument("--launcher", type=pathlib.Path, required=True)
        command.add_argument("--result", type=pathlib.Path, required=True)
        if mode == "build":
            command.add_argument("--run-label", required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    options = _parser().parse_args(argv)
    common = {
        "repository_root": options.repository_root,
        "artifact_store": options.cas,
        "outputs": options.outputs,
        "scratch": options.scratch,
        "gpgv": options.gpgv,
        "zstd": options.zstd,
        "launcher": options.launcher,
    }
    try:
        if options.mode == "preflight":
            document = preflight(**common)
        else:
            document = build(
                result=options.result,
                run_label=options.run_label,
                **common,
            )
        if options.mode == "preflight":
            _publish_result(options.result, document)
        print(
            "native-shadow closed-local image-to-readiness: "
            f"{options.mode} PASS"
        )
        return 0
    except (ClosedLocalImageError, sealed.SuccessorProduceV5Error, OSError) as exc:
        print(
            f"native-shadow closed-local image-to-readiness: FAIL: {exc}",
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
