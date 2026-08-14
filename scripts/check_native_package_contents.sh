#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <package-staging-root>" >&2
  exit 2
fi

root="$1"
required_files=(
  "opt/log73/bin/log73-backend"
  "opt/log73/bin/log73-launcher"
  "opt/log73/bin/log73-radio-client"
  "usr/share/applications/log73.desktop"
  "usr/share/applications/log73-radio-client.desktop"
  "usr/share/icons/hicolor/512x512/apps/log73.png"
  "usr/share/icons/hicolor/512x512/apps/log73-radio-client.png"
)

for relative_path in "${required_files[@]}"; do
  path="${root}/${relative_path}"
  [[ -f "$path" ]] || {
    echo "missing package file: ${relative_path}" >&2
    exit 1
  }
done

[[ -x "${root}/opt/log73/bin/log73-radio-client" ]] || {
  echo "Radio Client package binary is not executable" >&2
  exit 1
}
grep -Fq 'Name=Log73 Radio Client' "${root}/usr/share/applications/log73-radio-client.desktop"
grep -Fq 'Exec=/opt/log73/bin/log73-radio-client' "${root}/usr/share/applications/log73-radio-client.desktop"
echo "native package contents are complete"
