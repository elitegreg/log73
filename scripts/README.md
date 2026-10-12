# Test data scripts

## Backend latency benchmark

Build the production frontend and optimized backend, then run:

```bash
pnpm run build:production
cargo build -p log73-launcher --bin log73-backend --profile dist
node scripts/benchmark_backend.mjs --samples 24 --verify-cache --output /tmp/log73-latency.json
```

On Windows, use an output path such as `log73-latency.json`; the binary defaults
to `target/dist/log73-backend.exe`. Use `--binary <path>` to benchmark another
build, including an installed release. Run from a source checkout with the
matching packaged data files.

The script starts its own backend on an available localhost port using a fresh
temporary database and generated credentials. It measures completed responses
for `/api/modes`, `/api/bands`, and `/api/logs` sequentially and with eight
concurrent requests, with login disabled and with a warmed Basic Auth cache.
It also measures the first password verification, QSO commits, and lightweight
requests during audio/serial enumeration. It restarts the private backend to
measure a cold, concurrent contact-cache fill, then exercises warm configuration,
radio/log records, counts, stats, contact searches, ADIF/Cabrillo exports, serial
state, durable reservations, and cached reads during concurrent writes.
The fixture contains 1,000 contacts by default, seeded through API batches of
at most 100; use `--contacts <count>` to change the log size. The initial commit
sample is retained even when the requested fixture size is smaller than `--samples`.
It stops its backend and removes only
its temporary fixture directory, leaving installed data untouched.

The default log filter is `info,log73_backend::auth=debug`: request summaries and
password-verification counts are captured without enabling verbose logging for
every subsystem. Use `--log-level debug` when detailed operation events are needed.

The JSON report contains p50/p95/max latency, the number of actual password
verifications, and SQLite settings. The adjacent `.log` file preserves stage
timings for diagnosis. The new backend should verify the generated password
exactly once across the cold and warm-auth workloads. Use the same hardware,
sample count, log level, and optimized profile for comparisons. No measurements
are taken against an existing service or production database.

The report's `db_commands_by_request` groups request-summary cache and worker
counters by HTTP method and path. `--verify-cache` requires the updated backend
and info-level request logs: exactly one read command/load is expected for the
cold contact fill; other reads and exports must issue zero worker commands.
Omit this flag when measuring older binaries without cache counters. This is a
database-access check, not a wall-clock latency assertion.

Windows acceptance targets are lightweight request p95 below 100 ms when idle
and below 250 ms during enumeration. These are manual performance targets, not
wall-clock CI assertions. Stage timings, including auth-disabled measurements,
must identify any remaining outliers before claiming a Windows latency fix.

See [recorded measurements and remaining verification](../docs/backend-performance.md).

## SC QSO Party (In State)

`gen_sc_qso_party_in_state.py` generates contacts for `SC-QSO-PARTY (In State)` **through the backend REST API**.

It does **not** write SQLite directly; backend must be running.

### Requirements

- Python 3.10+
- `requests` (`pip install requests`)
- Backend running (default: `http://127.0.0.1:7300`)

### Example

```bash
python3 scripts/gen_sc_qso_party_in_state.py \
  --base-url http://127.0.0.1:7300 \
  --station-callsign N0CALL \
  --my-county RICH \
  --qso-count 5000 \
  --supercheckpartial data/MASTER.SCP \
  --start "2026-06-01 00:00:00" \
  --hours 24
```

### Optional filters/auth

```bash
python3 scripts/gen_sc_qso_party_in_state.py \
  --base-url http://127.0.0.1:7300 \
  --station-callsign N0CALL \
  --my-county RICH \
  --qso-count 1000 \
  --supercheckpartial data/MASTER.SCP \
  --start 1767225600 \
  --hours 12 \
  --callsign-regex '^(K|N|W|VA|VE)' \
  --username admin
```

### Notes

- Callsigns are filtered to US/Canada-like prefixes: `K/N/W/AA-AL/VA/VE`.
- RST is always `599` sent/received.
- `SRX_STRING` behavior:
  - `VA/VE` stations send province code.
  - `K/N/W/AA-AL` stations send mostly state/province, occasionally SC county.
- Exchange is kept stable per callsign.
- Dupes are allowed but capped at ~2%.
