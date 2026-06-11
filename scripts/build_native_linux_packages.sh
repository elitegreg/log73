#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <rust-target> <version>" >&2
  exit 2
fi

target="$1"
version="$2"
nfpm_bin="${NFPM:-nfpm}"

case "$target" in
  x86_64-unknown-linux-gnu)
    nfpm_arch="amd64"
    artifact_arch="x86_64"
    ;;
  aarch64-unknown-linux-gnu)
    nfpm_arch="arm64"
    artifact_arch="aarch64"
    ;;
  *)
    echo "unsupported Linux target for native packaging: $target" >&2
    exit 2
    ;;
esac

if ! command -v "$nfpm_bin" >/dev/null 2>&1; then
  echo "nfpm is required to build native Linux packages" >&2
  echo "install with: go install github.com/goreleaser/nfpm/v2/cmd/nfpm@v2.43.1" >&2
  echo "or set NFPM=/path/to/nfpm" >&2
  exit 1
fi

binary_dir="target/${target}/dist"
backend_bin="${binary_dir}/log73-backend"
launcher_bin="${binary_dir}/log73-launcher"

if [[ ! -x "$backend_bin" || ! -x "$launcher_bin" ]]; then
  echo "expected dist binaries were not found under ${binary_dir}" >&2
  echo "run: dist build --artifacts=local --target=${target}" >&2
  exit 1
fi

package_root="target/native-packages/${target}/root"
output_dir="target/native-packages"
config_path="target/native-packages/${target}/nfpm.yaml"

rm -rf "${package_root}"
mkdir -p \
  "${package_root}/opt/log73/bin" \
  "${package_root}/opt/log73/data" \
  "${package_root}/usr/share/applications" \
  "${package_root}/usr/share/icons/hicolor/512x512/apps" \
  "${output_dir}"

install -m 0755 "$backend_bin" "${package_root}/opt/log73/bin/log73-backend"
install -m 0755 "$launcher_bin" "${package_root}/opt/log73/bin/log73-launcher"
install -m 0644 data/MASTER.SCP "${package_root}/opt/log73/data/MASTER.SCP"
install -m 0644 data/cty.dat "${package_root}/opt/log73/data/cty.dat"
cp -R data/contest-rules "${package_root}/opt/log73/data/contest-rules"
find "${package_root}/opt/log73/data/contest-rules" -type f -exec chmod 0644 {} +
install -m 0644 static/log73-icon-512.png \
  "${package_root}/usr/share/icons/hicolor/512x512/apps/log73.png"

cat > "${package_root}/usr/share/applications/log73.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Log73
Comment=Amateur radio contest logger
Exec=/opt/log73/bin/log73-launcher
Icon=log73
Terminal=false
Categories=Utility;HamRadio;
DESKTOP

write_config() {
  local config_path="$1"
  local chromium_dep="$2"
  local udev_dep="$3"

  cat > "$config_path" <<EOF
name: log73
arch: ${nfpm_arch}
platform: linux
version: ${version}
section: utils
priority: optional
maintainer: Log73 Maintainers <maintainers@log73.local>
description: |
  Log73 amateur radio contest logger.
vendor: Log73
homepage: https://github.com/elitegreg/log73
license: MIT OR Apache-2.0
contents:
  - src: ${package_root}/opt/log73/bin/log73-backend
    dst: /opt/log73/bin/log73-backend
    file_info:
      mode: 0755
  - src: ${package_root}/opt/log73/bin/log73-launcher
    dst: /opt/log73/bin/log73-launcher
    file_info:
      mode: 0755
  - src: ${package_root}/opt/log73/data/
    dst: /opt/log73/data
  - src: ${package_root}/usr/share/applications/log73.desktop
    dst: /usr/share/applications/log73.desktop
  - src: ${package_root}/usr/share/icons/hicolor/512x512/apps/log73.png
    dst: /usr/share/icons/hicolor/512x512/apps/log73.png
depends:
  - ${udev_dep}
  - ${chromium_dep}
EOF
}

deb_config_path="${config_path}.deb"
rpm_config_path="${config_path}.rpm"
write_config "$deb_config_path" chromium-browser libudev1
write_config "$rpm_config_path" chromium systemd-libs

"$nfpm_bin" package --packager deb --config "$deb_config_path" \
  --target "${output_dir}/log73_${version}_${artifact_arch}.deb"
"$nfpm_bin" package --packager rpm --config "$rpm_config_path" \
  --target "${output_dir}/log73-${version}-1.${artifact_arch}.rpm"
