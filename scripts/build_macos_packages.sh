#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <rust-target> <version>" >&2
  exit 2
fi

target="$1"
version="$2"

case "$target" in
  aarch64-apple-darwin|x86_64-apple-darwin) ;;
  *) echo "unsupported macOS target for native packaging: $target" >&2; exit 2 ;;
esac

if ! command -v pkgbuild >/dev/null 2>&1; then
  echo "pkgbuild is required to build macOS packages" >&2
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

output_dir="target/macos-packages"
stage_dir="${output_dir}/${target}"
launcher_package_root="${stage_dir}/launcher-package"
radio_client_package_root="${stage_dir}/radio-client-package"
launcher_app="${launcher_package_root}/log73-launcher.app"
radio_client_app="${radio_client_package_root}/log73-radio-client.app"
rm -rf "$stage_dir"
mkdir -p "$launcher_app/Contents/MacOS" "$launcher_app/Contents/Helpers" \
  "$launcher_app/Contents/Resources/data" \
  "$radio_client_app/Contents/MacOS" "$radio_client_app/Contents/Resources"

install -m 0755 "$launcher_bin" "$launcher_app/Contents/MacOS/log73-launcher"
install -m 0755 "$backend_bin" "$launcher_app/Contents/Helpers/log73-backend"
cp -R data/. "$launcher_app/Contents/Resources/data/"
find "$launcher_app/Contents/Resources/data" -type f -exec chmod 0644 {} +
install -m 0755 "$radio_client_bin" "$radio_client_app/Contents/MacOS/log73-radio-client"

create_icon() {
  local app="$1"
  local iconset="${app}/Contents/Resources/log73.iconset"
  mkdir -p "$iconset"
  sips -z 16 16 static/log73-icon-512.png --out "$iconset/icon_16x16.png" >/dev/null
  sips -z 32 32 static/log73-icon-512.png --out "$iconset/icon_16x16@2x.png" >/dev/null
  sips -z 32 32 static/log73-icon-512.png --out "$iconset/icon_32x32.png" >/dev/null
  sips -z 64 64 static/log73-icon-512.png --out "$iconset/icon_32x32@2x.png" >/dev/null
  sips -z 128 128 static/log73-icon-512.png --out "$iconset/icon_128x128.png" >/dev/null
  sips -z 256 256 static/log73-icon-512.png --out "$iconset/icon_128x128@2x.png" >/dev/null
  sips -z 256 256 static/log73-icon-512.png --out "$iconset/icon_256x256.png" >/dev/null
  sips -z 512 512 static/log73-icon-512.png --out "$iconset/icon_256x256@2x.png" >/dev/null
  sips -z 512 512 static/log73-icon-512.png --out "$iconset/icon_512x512.png" >/dev/null
  sips -z 1024 1024 static/log73-icon-512.png --out "$iconset/icon_512x512@2x.png" >/dev/null
  iconutil -c icns "$iconset" -o "${app}/Contents/Resources/log73.icns"
  rm -rf "$iconset"
}

create_icon "$launcher_app"
create_icon "$radio_client_app"

write_info_plist() {
  local path="$1" display_name="$2" executable="$3" identifier="$4"
  cat > "$path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>${display_name}</string>
  <key>CFBundleExecutable</key>
  <string>${executable}</string>
  <key>CFBundleIdentifier</key>
  <string>${identifier}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleIconFile</key>
  <string>log73</string>
  <key>CFBundleName</key>
  <string>${display_name}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${version}</string>
  <key>CFBundleVersion</key>
  <string>${version}</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
</dict>
</plist>
EOF
}

write_info_plist "$launcher_app/Contents/Info.plist" log73-launcher log73-launcher com.log73.launcher
write_info_plist "$radio_client_app/Contents/Info.plist" log73-radio-client log73-radio-client com.log73.radio-client

mkdir -p "$output_dir"
pkgbuild --root "$launcher_package_root" --identifier com.log73.launcher --version "$version" \
  --install-location /Applications "${output_dir}/log73-launcher-${version}-${target}.pkg"
pkgbuild --root "$radio_client_package_root" --identifier com.log73.radio-client --version "$version" \
  --install-location /Applications "${output_dir}/log73-radio-client-${version}-${target}.pkg"
