#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY_PATH="${OWNSTACK_BINARY_PATH:-${ROOT_DIR}/target/release-lto/ownstack-ide}"
OUTPUT_PATH="${OWNSTACK_APPIMAGE_OUTPUT:-${ROOT_DIR}/OwnStack-linux-x86_64.AppImage}"
APPDIR="${ROOT_DIR}/target/appimage/OwnStack.AppDir"
APPIMAGETOOL="${ROOT_DIR}/target/appimage/appimagetool-x86_64.AppImage"

if [[ ! -f "${BINARY_PATH}" ]]; then
  echo "Missing binary: ${BINARY_PATH}" >&2
  exit 1
fi

mkdir -p "${APPDIR}/usr/bin"
mkdir -p "${APPDIR}/usr/share/applications"
mkdir -p "${APPDIR}/usr/share/metainfo"
mkdir -p "${APPDIR}/usr/share/icons/hicolor/512x512/apps"

install -Dm755 "${BINARY_PATH}" "${APPDIR}/usr/bin/ownstack-ide"
install -Dm644 "${ROOT_DIR}/extra/linux/io.ownstack.ownstackide.desktop" \
  "${APPDIR}/usr/share/applications/io.ownstack.ownstackide.desktop"
install -Dm644 "${ROOT_DIR}/extra/linux/io.ownstack.ownstackide.metainfo.xml" \
  "${APPDIR}/usr/share/metainfo/io.ownstack.ownstackide.metainfo.xml"
install -Dm644 "${ROOT_DIR}/extra/images/logo.png" \
  "${APPDIR}/usr/share/icons/hicolor/512x512/apps/io.ownstack.ownstackide.png"
install -Dm644 "${ROOT_DIR}/extra/images/logo.png" \
  "${APPDIR}/io.ownstack.ownstackide.png"
cp "${ROOT_DIR}/extra/linux/io.ownstack.ownstackide.desktop" \
  "${APPDIR}/io.ownstack.ownstackide.desktop"

cat > "${APPDIR}/AppRun" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
HERE="$(dirname "$(readlink -f "${0}")")"
exec "${HERE}/usr/bin/ownstack-ide" "$@"
EOF
chmod +x "${APPDIR}/AppRun"

if [[ ! -x "${APPIMAGETOOL}" ]]; then
  mkdir -p "$(dirname "${APPIMAGETOOL}")"
  curl -fsSL -o "${APPIMAGETOOL}" \
    "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
  chmod +x "${APPIMAGETOOL}"
fi

ARCH=x86_64 "${APPIMAGETOOL}" --appimage-extract-and-run "${APPDIR}" "${OUTPUT_PATH}"
chmod +x "${OUTPUT_PATH}"
echo "Created AppImage: ${OUTPUT_PATH}"
