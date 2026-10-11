#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <macOS-package-staging-root>" >&2
  exit 2
fi

root="$1"
launcher_app="${root}/launcher-package/log73-launcher.app"
radio_client_app="${root}/radio-client-package/log73-radio-client.app"

for path in \
  "$launcher_app/Contents/Info.plist" \
  "$launcher_app/Contents/MacOS/log73-launcher" \
  "$launcher_app/Contents/Helpers/log73-backend" \
  "$launcher_app/Contents/Resources/log73.icns" \
  "$launcher_app/Contents/Resources/data/cty.csv" \
  "$radio_client_app/Contents/Info.plist" \
  "$radio_client_app/Contents/Resources/log73.icns" \
  "$radio_client_app/Contents/MacOS/log73-radio-client"; do
  [[ -f "$path" ]] || { echo "missing macOS package file: ${path}" >&2; exit 1; }
done

[[ -x "$launcher_app/Contents/MacOS/log73-launcher" ]]
[[ -x "$launcher_app/Contents/Helpers/log73-backend" ]]
[[ -x "$radio_client_app/Contents/MacOS/log73-radio-client" ]]
[[ ! -e "$radio_client_app/Contents/Resources/data" ]]
diff --brief -r data "$launcher_app/Contents/Resources/data"
echo "macOS package contents are complete"
