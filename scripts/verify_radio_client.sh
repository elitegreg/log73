#!/usr/bin/env bash
set -euo pipefail

echo "== Rust/frontend CI =="
make ci

echo "== Debug binaries =="
make all

echo "== Native package staging =="
make package-smoke

echo "== Registration and lifecycle fixture tests =="
cargo test -p log73-radio-client local_http_fixture_round_trips_registration_heartbeat_and_offline
cargo test -p log73-radio-client lifecycle::tests::dummy_radio_host_binds_and_stops

if command -v dist >/dev/null 2>&1; then
  echo "== cargo-dist manifest =="
  manifest="$(dist plan --allow-dirty --output-format=json)"
  grep -Fq 'log73-radio-client' <<<"$manifest" || {
    echo "cargo-dist manifest does not include log73-radio-client" >&2
    exit 1
  }
else
  echo "cargo-dist not installed; release artifact smoke check deferred"
fi

cat <<'EOF'
Automated checks passed.
Deferred manual checks: physical CAT/serial/Winkeyer/audio hardware, WSJT-X
UDP interoperability with an actual WSJT-X process, and native installer
launch tests on Windows/macOS. Run those checks on each target platform before
publishing a release.
EOF
