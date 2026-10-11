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
  x86_64-unknown-linux-gnu) nfpm_arch="amd64"; artifact_arch="x86_64" ;;
  aarch64-unknown-linux-gnu) nfpm_arch="arm64"; artifact_arch="aarch64" ;;
  *) echo "unsupported Linux target for native packaging: $target" >&2; exit 2 ;;
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
radio_client_bin="${binary_dir}/log73-radio-client"
if [[ ! -x "$backend_bin" || ! -x "$launcher_bin" || ! -x "$radio_client_bin" ]]; then
  echo "expected dist binaries were not found under ${binary_dir}" >&2
  echo "run: dist build --artifacts=local --target=${target}" >&2
  exit 1
fi

base_root="target/native-packages/${target}"
package_root="${base_root}/root"
output_dir="target/native-packages"
mkdir -p "$output_dir"
rm -rf "$package_root"

mkdir -p "$package_root/opt/log73/bin" "$package_root/opt/log73/data" \
  "$package_root/usr/share/applications" \
  "$package_root/usr/share/icons/hicolor/512x512/apps"
install -m 0755 "$backend_bin" "$package_root/opt/log73/bin/log73-backend"
install -m 0755 "$launcher_bin" "$package_root/opt/log73/bin/log73-launcher"
install -m 0755 "$radio_client_bin" "$package_root/opt/log73/bin/log73-radio-client"
cp -R data/. "$package_root/opt/log73/data/"
find "$package_root/opt/log73/data" -type f -exec chmod 0644 {} +
install -m 0644 static/log73-icon-512.png "$package_root/usr/share/icons/hicolor/512x512/apps/log73-launcher.png"
install -m 0644 static/log73-icon-512.png "$package_root/usr/share/icons/hicolor/512x512/apps/log73-radio-client.png"
cat > "$package_root/usr/share/applications/log73-launcher.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=log73-launcher
Comment=Amateur radio contest logger
Exec=/opt/log73/bin/log73-launcher
Icon=log73-launcher
Terminal=false
Categories=Utility;HamRadio;
DESKTOP

cat > "$package_root/usr/share/applications/log73-radio-client.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=log73-radio-client
Comment=Client-side radio controller for Log73
Exec=/opt/log73/bin/log73-radio-client
Icon=log73-radio-client
Terminal=false
Categories=Utility;HamRadio;
DESKTOP

write_launcher_config() {
  local config_path="$1" chromium_dep="$2" udev_dep="$3"
  cat > "$config_path" <<EOF
name: log73-launcher
arch: ${nfpm_arch}
platform: linux
version: ${version}
section: utils
priority: optional
maintainer: Log73 Maintainers <maintainers@log73.local>
description: |
  Log73 contest logger launcher and backend.
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
  - src: ${package_root}/usr/share/applications/log73-launcher.desktop
    dst: /usr/share/applications/log73-launcher.desktop
  - src: ${package_root}/usr/share/icons/hicolor/512x512/apps/log73-launcher.png
    dst: /usr/share/icons/hicolor/512x512/apps/log73-launcher.png
depends:
  - ${udev_dep}
  - ${chromium_dep}
EOF
}

write_radio_client_config() {
  local config_path="$1" udev_dep="$2"
  cat > "$config_path" <<EOF
name: log73-radio-client
arch: ${nfpm_arch}
platform: linux
version: ${version}
section: utils
priority: optional
maintainer: Log73 Maintainers <maintainers@log73.local>
description: |
  Standalone client-side radio controller for Log73.
vendor: Log73
homepage: https://github.com/elitegreg/log73
license: MIT OR Apache-2.0
contents:
  - src: ${package_root}/opt/log73/bin/log73-radio-client
    dst: /opt/log73/bin/log73-radio-client
    file_info:
      mode: 0755
  - src: ${package_root}/usr/share/applications/log73-radio-client.desktop
    dst: /usr/share/applications/log73-radio-client.desktop
  - src: ${package_root}/usr/share/icons/hicolor/512x512/apps/log73-radio-client.png
    dst: /usr/share/icons/hicolor/512x512/apps/log73-radio-client.png
depends:
  - ${udev_dep}
EOF
}

for package in deb rpm; do
  if [[ "$package" == deb ]]; then
    chromium_dep="chromium-browser"
    udev_dep="libudev1"
  else
    chromium_dep="chromium"
    udev_dep="systemd-libs"
  fi
  launcher_config="${base_root}/log73-launcher.${package}.yaml"
  radio_client_config="${base_root}/log73-radio-client.${package}.yaml"
  write_launcher_config "$launcher_config" "$chromium_dep" "$udev_dep"
  write_radio_client_config "$radio_client_config" "$udev_dep"
  if [[ "$package" == deb ]]; then
    "$nfpm_bin" package --packager deb --config "$launcher_config" --target "${output_dir}/log73-launcher_${version}_${artifact_arch}.deb"
    "$nfpm_bin" package --packager deb --config "$radio_client_config" --target "${output_dir}/log73-radio-client_${version}_${artifact_arch}.deb"
  else
    "$nfpm_bin" package --packager rpm --config "$launcher_config" --target "${output_dir}/log73-launcher-${version}-1.${artifact_arch}.rpm"
    "$nfpm_bin" package --packager rpm --config "$radio_client_config" --target "${output_dir}/log73-radio-client-${version}-1.${artifact_arch}.rpm"
  fi
done
