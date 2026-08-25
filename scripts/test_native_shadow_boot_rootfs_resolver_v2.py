#!/usr/bin/env python3
"""RED-first contracts for the ARM64 boot-rootfs dependency resolver v2."""

from __future__ import annotations

import hashlib
import pathlib
import unittest

from scripts import native_shadow_boot_rootfs_resolver_v2 as resolver


ROOT = pathlib.Path(__file__).resolve().parents[1]


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def stanza(
    name: str,
    *,
    version: str = "1",
    architecture: str = "arm64",
    depends: str = "",
    pre_depends: str = "",
    provides: str = "",
    multi_arch: str = "",
) -> bytes:
    payload = f"deb:{name}:{version}:{architecture}".encode()
    fields = [
        f"Package: {name}",
        f"Version: {version}",
        f"Architecture: {architecture}",
        f"Filename: pool/main/{name[0]}/{name}/{name}_{version}_{architecture}.deb",
        f"Size: {len(payload)}",
        f"SHA256: {_sha(payload)}",
    ]
    if depends:
        fields.append(f"Depends: {depends}")
    if pre_depends:
        fields.append(f"Pre-Depends: {pre_depends}")
    if provides:
        fields.append(f"Provides: {provides}")
    if multi_arch:
        fields.append(f"Multi-Arch: {multi_arch}")
    return ("\n".join(fields) + "\n").encode()


class NativeShadowBootRootfsResolverV2Tests(unittest.TestCase):
    def resolve(
        self,
        rows: list[bytes],
        seeds: list[str],
        *,
        pins: dict[str, str] | None = None,
    ) -> dict:
        return resolver.resolve_package_closure_v2(
            b"\n".join(rows),
            seeds,
            "noble-main",
            "main",
            target_os="linux",
            target_architecture="arm64",
            virtual_provider_pins=pins or {},
        )

    def test_native_multiarch_qualifier_and_debian_less_than_are_supported(self) -> None:
        result = self.resolve(
            [
                stanza("app", depends="python3:any, helper (<< 3.13)"),
                stanza("python3", version="3.12.3-0ubuntu2", multi_arch="allowed"),
                stanza("helper", version="3.12.9"),
            ],
            ["app"],
        )
        self.assertEqual(
            {row["name"] for row in result["packages"]},
            {"app", "helper", "python3"},
        )

    def test_pre_depends_is_part_of_the_transitive_runtime_closure(self) -> None:
        result = self.resolve(
            [
                stanza("app", pre_depends="bootstrap"),
                stanza("bootstrap", depends="runtime"),
                stanza("runtime"),
            ],
            ["app"],
        )
        self.assertEqual(
            {row["name"] for row in result["packages"]},
            {"app", "bootstrap", "runtime"},
        )
        app = next(row for row in result["packages"] if row["name"] == "app")
        self.assertEqual(app["dependencyResolutions"][0]["field"], "Pre-Depends")

    def test_any_qualifier_does_not_bypass_the_packages_multiarch_declaration(self) -> None:
        with self.assertRaisesRegex(resolver.ResolverV2Error, "unresolved"):
            self.resolve(
                [stanza("app", depends="python3:any"), stanza("python3")],
                ["app"],
            )
        result = self.resolve(
            [stanza("app", depends="python3:native"), stanza("python3")],
            ["app"],
        )
        self.assertEqual(
            {row["name"] for row in result["packages"]},
            {"app", "python3"},
        )

    def test_architecture_restrictions_are_evaluated_for_linux_arm64(self) -> None:
        result = self.resolve(
            [
                stanza(
                    "app",
                    depends=(
                        "arm-only [arm64], ignored [amd64], "
                        "linux-only [linux-any], excluded [!arm64]"
                    ),
                ),
                stanza("arm-only"),
                stanza("ignored"),
                stanza("linux-only"),
                stanza("excluded"),
            ],
            ["app"],
        )
        self.assertEqual(
            {row["name"] for row in result["packages"]},
            {"app", "arm-only", "linux-only"},
        )
        with self.assertRaisesRegex(resolver.ResolverV2Error, "mix"):
            self.resolve(
                [
                    stanza("mixed", depends="helper [arm64 !amd64]"),
                    stanza("helper"),
                ],
                ["mixed"],
            )

    def test_ambiguous_virtual_dependency_requires_an_explicit_provider_pin(self) -> None:
        rows = [
            stanza("app", depends="awk"),
            stanza("gawk", provides="awk"),
            stanza("mawk", provides="awk"),
        ]
        with self.assertRaisesRegex(resolver.ResolverV2Error, "ambiguous"):
            self.resolve(rows, ["app"])
        result = self.resolve(rows, ["app"], pins={"awk": "mawk"})
        self.assertEqual(
            {row["name"] for row in result["packages"]},
            {"app", "mawk"},
        )

    def test_unused_or_invalid_provider_pins_fail_closed(self) -> None:
        rows = [stanza("app"), stanza("mawk", provides="awk")]
        with self.assertRaisesRegex(resolver.ResolverV2Error, "unused provider pin"):
            self.resolve(rows, ["app"], pins={"awk": "mawk"})
        with self.assertRaisesRegex(resolver.ResolverV2Error, "provider pin"):
            self.resolve(
                [stanza("app", depends="awk"), stanza("mawk", provides="awk")],
                ["app"],
                pins={"awk": "missing"},
            )

    def test_provider_pin_object_order_does_not_change_the_resolution(self) -> None:
        rows = [
            stanza("app", depends="awk, shell"),
            stanza("mawk", provides="awk"),
            stanza("gawk", provides="awk"),
            stanza("dash", provides="shell"),
            stanza("busybox", provides="shell"),
        ]
        first = self.resolve(
            rows,
            ["app"],
            pins={"awk": "mawk", "shell": "dash"},
        )
        second = self.resolve(
            rows,
            ["app"],
            pins={"shell": "dash", "awk": "mawk"},
        )
        self.assertEqual(resolver.canonical_json(first), resolver.canonical_json(second))

    def test_direct_package_version_cannot_be_spoofed_by_self_provides(self) -> None:
        with self.assertRaisesRegex(resolver.ResolverV2Error, "unresolved"):
            self.resolve(
                [
                    stanza("app", depends="runtime (= 999)"),
                    stanza("runtime", version="1", provides="runtime (= 999)"),
                ],
                ["app"],
            )

    def test_duplicate_virtual_provides_names_fail_independent_of_field_order(self) -> None:
        for provides in ("virt (= 1), virt (= 2)", "virt (= 2), virt (= 1)"):
            with self.subTest(provides=provides):
                with self.assertRaisesRegex(
                    resolver.ResolverV2Error, "duplicate provided name"
                ):
                    self.resolve(
                        [
                            stanza("app", depends="virt (= 1)"),
                            stanza("provider", provides=provides),
                        ],
                        ["app"],
                    )

    def test_build_profiles_and_foreign_architecture_candidates_are_rejected(self) -> None:
        with self.assertRaisesRegex(resolver.ResolverV2Error, "build profile"):
            self.resolve(
                [stanza("app", depends="helper <!nocheck>"), stanza("helper")],
                ["app"],
            )
        with self.assertRaisesRegex(resolver.ResolverV2Error, "architecture"):
            self.resolve(
                [stanza("app"), stanza("foreign", architecture="amd64")],
                ["app", "foreign"],
            )

    def test_result_is_order_independent_and_records_the_v2_policy(self) -> None:
        rows = [
            stanza("app", depends="runtime | fallback"),
            stanza("runtime"),
            stanza("fallback"),
        ]
        first = self.resolve(rows, ["app"])
        second = self.resolve(list(reversed(rows)), ["app"])
        self.assertEqual(resolver.canonical_json(first), resolver.canonical_json(second))
        self.assertEqual(
            first["policy"],
            {
                "architectureRestrictionEvaluation": "linux-arm64",
                "dependencyFields": ["Depends", "Pre-Depends"],
                "foreignArchitectureSelection": "forbidden",
                "multiArchQualifier": "native-index-only",
                "providerSelection": "direct-then-explicit-pin-else-stop",
            },
        )

    def test_self_test_runs_the_boot_rootfs_resolver_v2_suite(self) -> None:
        body = (ROOT / "scripts/self-test.sh").read_text(encoding="utf-8")
        self.assertIn(
            "scripts/test_native_shadow_boot_rootfs_resolver_v2.py",
            body,
        )


if __name__ == "__main__":
    unittest.main()
