#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || -z "${1:-}" || "$1" == "/" ]]; then
  echo "usage: $0 <empty-install-prefix>" >&2
  exit 2
fi
if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  echo "native checker CI toolchain installer supports Linux x86_64 only" >&2
  exit 2
fi

prefix="$1"
rustc_commit_hash="e7795af6d2449fb05a6393c3320ced873a999eb3"
cargo_commit_hash="3efb1f477e99b42974b982d939fd100303cdf7db"
base="https://ci-artifacts.rust-lang.org/rustc-builds/${rustc_commit_hash}"
scratch="$(mktemp -d "${RUNNER_TEMP:-/tmp}/boole-native-toolchain.XXXXXX")"

cleanup() {
  rm -rf "$scratch"
}
trap cleanup EXIT

download_and_verify() {
  local archive="$1"
  local digest="$2"
  curl --proto '=https' --tlsv1.2 --retry 5 --location --silent --show-error \
    --fail "${base}/${archive}" -o "${scratch}/${archive}"
  printf '%s  %s\n' "$digest" "${scratch}/${archive}" | sha256sum -c -
  tar -xJf "${scratch}/${archive}" -C "$scratch"
}

download_and_verify \
  rustc-nightly-x86_64-unknown-linux-gnu.tar.xz \
  12cd470422b39da22a7b8c2f069c25e66200d5a46c1be5dac0bfe7620ed0d415
download_and_verify \
  rust-std-nightly-x86_64-unknown-linux-gnu.tar.xz \
  fd04194fb361ef69735a0b722fcaf6d9b49a339944f485aebcc4c172adb5c339
download_and_verify \
  cargo-nightly-x86_64-unknown-linux-gnu.tar.xz \
  53e718c828a16746abdf3f8fb6f4c75ce5494a6f547ef6f02d45d72faef4c426

mkdir -p "$prefix"
"${scratch}/rustc-nightly-x86_64-unknown-linux-gnu/install.sh" \
  --prefix="$prefix" --disable-ldconfig
"${scratch}/rust-std-nightly-x86_64-unknown-linux-gnu/install.sh" \
  --prefix="$prefix" --disable-ldconfig
"${scratch}/cargo-nightly-x86_64-unknown-linux-gnu/install.sh" \
  --prefix="$prefix" --disable-ldconfig

rustc_info="$("${prefix}/bin/rustc" -vV)"
cargo_info="$("${prefix}/bin/cargo" -Vv)"
grep -Fqx "commit-hash: ${rustc_commit_hash}" <<<"$rustc_info"
grep -Fqx "commit-hash: ${cargo_commit_hash}" <<<"$cargo_info"
