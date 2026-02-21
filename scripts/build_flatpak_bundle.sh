#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY_PATH="${OWNSTACK_BINARY_PATH:-${ROOT_DIR}/target/release-lto/ownstack-ide}"
MANIFEST_PATH="${ROOT_DIR}/extra/linux/flatpak/io.ownstack.ownstackide.yml"
BUILD_DIR="${ROOT_DIR}/target/flatpak/build"
REPO_DIR="${ROOT_DIR}/target/flatpak/repo"
OUTPUT_BUNDLE="${OWNSTACK_FLATPAK_OUTPUT:-${ROOT_DIR}/OwnStack-linux-x86_64.flatpak}"

if [[ ! -f "${BINARY_PATH}" ]]; then
  echo "Missing binary: ${BINARY_PATH}" >&2
  exit 1
fi

mkdir -p "${ROOT_DIR}/target/release-lto"
install -Dm755 "${BINARY_PATH}" "${ROOT_DIR}/target/release-lto/ownstack-ide"

flatpak --user remote-add --if-not-exists flathub \
  https://flathub.org/repo/flathub.flatpakrepo
flatpak --user install -y flathub \
  org.freedesktop.Platform//23.08 org.freedesktop.Sdk//23.08

flatpak-builder --user --force-clean --repo="${REPO_DIR}" \
  "${BUILD_DIR}" "${MANIFEST_PATH}"
flatpak build-bundle "${REPO_DIR}" "${OUTPUT_BUNDLE}" \
  io.ownstack.ownstackide stable

echo "Created Flatpak bundle: ${OUTPUT_BUNDLE}"
