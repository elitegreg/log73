#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <launcher|radio-client> <package-staging-root>" >&2
  exit 2
fi

package="$1"
root="$2"

case "$package" in
  launcher)
    required_files=(
      "opt/log73/bin/log73-backend"
      "opt/log73/bin/log73-launcher"
      "opt/log73/data/cty.csv"
      "opt/log73/data/contest-rules/arrl-dx.yaml"
      "usr/share/applications/log73-launcher.desktop"
      "usr/share/icons/hicolor/512x512/apps/log73-launcher.png"
    )
    executable="opt/log73/bin/log73-launcher"
    desktop="usr/share/applications/log73-launcher.desktop"
    expected_exec="Exec=/opt/log73/bin/log73-launcher"
    ;;
  radio-client)
    required_files=(
      "opt/log73/bin/log73-radio-client"
      "usr/share/applications/log73-radio-client.desktop"
      "usr/share/icons/hicolor/512x512/apps/log73-radio-client.png"
    )
    executable="opt/log73/bin/log73-radio-client"
    desktop="usr/share/applications/log73-radio-client.desktop"
    expected_exec="Exec=/opt/log73/bin/log73-radio-client"
    ;;
  *)
    echo "unknown package: ${package}" >&2
    exit 2
    ;;
esac

for relative_path in "${required_files[@]}"; do
  path="${root}/${relative_path}"
  [[ -f "$path" ]] || {
    echo "missing package file: ${relative_path}" >&2
    exit 1
  }
done

[[ -x "${root}/${executable}" ]] || {
  echo "package binary is not executable: ${executable}" >&2
  exit 1
}
grep -Fq "Name=log73-${package}" "${root}/${desktop}"
grep -Fq "$expected_exec" "${root}/${desktop}"

if [[ "$package" == launcher ]]; then
  diff --brief -r data "${root}/opt/log73/data"
fi

echo "${package} native package contents are complete"
