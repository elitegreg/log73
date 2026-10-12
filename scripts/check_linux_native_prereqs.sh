#!/usr/bin/env bash
# Check Linux native build prerequisites for targets that compile radio-io.
#
# Probes compile-time metadata with pkg-config, which is what the -sys crate
# build scripts (libudev-sys, alsa-sys, pipewire) actually need. A runtime-only
# library (e.g. libudev.so.1 without libudev-dev) is still reported as missing.
set -euo pipefail

INSTALL_HINT="sudo apt-get install -y libudev-dev pkg-config libasound-dev libpipewire-0.3-dev"

# Only meaningful on Linux; these targets also build on macOS/Windows.
if [[ "$(uname -s)" != "Linux" ]]; then
  echo "check_linux_native_prereqs: skipping (not Linux)" >&2
  exit 0
fi

# Host metadata says nothing about a foreign sysroot when cross-compiling.
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  echo "check_linux_native_prereqs: skipping (cross-compiling for $CARGO_BUILD_TARGET)" >&2
  exit 0
fi

if ! command -v pkg-config >/dev/null 2>&1; then
  echo "error: pkg-config not found; native build prerequisites cannot be verified." >&2
  echo "Install them, e.g. on Debian/Ubuntu:" >&2
  echo "  $INSTALL_HINT" >&2
  exit 1
fi

# Pairs of "<pkg-config module>:<Debian/Ubuntu package>:<needed by>".
checks=(
  "libudev:libudev-dev:serial-port enumeration"
  "alsa:libasound-dev:audio output"
  "libpipewire-0.3:libpipewire-0.3-dev:PipeWire audio"
)

missing_modules=()
missing_packages=()
for check in "${checks[@]}"; do
  module="${check%%:*}"
  rest="${check#*:}"
  package="${rest%%:*}"
  if ! pkg-config --exists "$module"; then
    missing_modules+=("$module")
    missing_packages+=("$package")
  fi
done

if [[ "${#missing_modules[@]}" -gt 0 ]]; then
  echo "error: missing Linux native build prerequisites: ${missing_modules[*]}" >&2
  echo "Missing development packages (Debian/Ubuntu names): ${missing_packages[*]}" >&2
  echo "Install them, e.g.:" >&2
  echo "  $INSTALL_HINT" >&2
  exit 1
fi
