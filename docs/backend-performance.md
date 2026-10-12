# Backend latency verification

The Windows report concerns slow backend requests with login disabled. Repeated
Argon2 verification is a separate confirmed cost when login is enabled; it cannot
explain an auth-disabled Windows request. Device enumeration, filesystem work,
console output, database commits, and scheduling must be distinguished using the
new request and worker-stage timings.

## Local measurements

An isolated Linux run on 2026-10-12 used optimized binaries, eight samples per
scenario, eight concurrent requests, and the log filter
`info,log73_backend::auth=debug`. The baseline binary was downloaded from the
published `v0.3.0-rc.2` launcher archive. Both builds used matching source-checkout
data, fresh temporary databases, and the same benchmark driver.

| Workload | rc.2 baseline | Updated backend |
| --- | --- | --- |
| Login-disabled `/modes`, sequential p50/p95 | 63 / 1211 ms | 45 / 468 ms |
| Login-disabled `/modes`, eight concurrent requests | Timed out after 30 seconds | Completed; p95 2904 ms |
| Warm-auth `/modes`, sequential p50/p95 | Not reached because baseline timed out | 45 / 84 ms |
| Warm-auth `/modes`, concurrent p50/p95 | Not reached | 117 / 137 ms |
| Warm-auth QSO commits, sequential p50/p95 | Not reached | 65 / 66 ms |

The updated run completed all scenarios and logged exactly one password
verification across its cold/warm-auth workloads. Warm cache checks took about
0.03–0.04 ms in request logs; database execution during QSO commits was generally
0.1–0.6 ms. SQLite reported WAL with FULL synchronous durability on Linux.

These are small diagnostic samples, not a claim of Windows performance or a
reliable throughput estimate. Client-measured time includes connection setup and
scheduling. Login-disabled outliers remain in this environment, and the Windows
acceptance targets have not been verified. Match slow requests to their stage
timings before attributing the remaining delay to a subsystem.

## Windows verification

Run the [isolated benchmark](../scripts/README.md#backend-latency-benchmark) with
24 or more samples against the new optimized Windows binary. Confirm SQLite
reports WAL/NORMAL, password verification occurs once, and cached checks invoke
Argon2 zero additional times. Compare lightweight requests with login disabled,
with warm authentication, during serial/audio enumeration, and alongside QSO
commits. Preserve the JSON and adjacent log file.

Target lightweight-request p95 below 100 ms when idle and below 250 ms during
enumeration. If those targets are missed, inspect auth, handler, DB queue/execute,
and blocking queue/execute timings and profile the remaining stage on the affected
machine. Handler time includes DB and blocking stages; those fields are not
additive. Database NORMAL on Windows intentionally trades recent-write durability
across OS crashes/power failures for commit speed.

## Complete database cache coverage

An additional isolated Linux comparison on 2026-10-12 used optimized `dist`
binaries before and after the database-cache expansion, a 1,000-contact fixture,
eight samples per measurement, and eight concurrent reads. Both used the same
expanded benchmark and matching source-checkout data. The backend was restarted
after seeding to exercise a cold contact-cache load. The baseline here is the
earlier uncommitted latency implementation, not the published rc.2 binary.

| Workload, p50 / p95 | Before cache expansion | After cache expansion |
| --- | --- | --- |
| Configuration, sequential | 73 / 92 ms | 59 / 68 ms |
| QSO count, concurrent | 165 / 291 ms | 122 / 141 ms |
| Counts alongside writes | 159 / 241 ms | 146 / 176 ms |
| ADIF exports, two concurrent requests | 169 / 308 ms | 140 / 155 ms |
| Serial reservations, concurrent | 180 / 246 ms | 152 / 282 ms |

The deterministic acceptance check passed: one coalesced cold contact load/read
command, zero worker commands for other reads and exports, and 45 write commands
for fixture creation, QSO batches, reservations, and configuration changes.
Separate SQL-traced tests confirm warm persistence reads, validation, and exports
issue no SQL, and warm contact writes/reservations require no SELECT readbacks or
serial scans. Request-summary counters count commands, not SQL statements.
Cancellation and revision tests also cover stale derived-state recovery and
authentication changes committed without the configuration handler's follow-up.

Latency remains noisy: serial-reservation p95 did not improve, and cold/startup
requests still showed multi-second client-measured outliers. This verifies reduced
database work, not a general Windows latency fix or a throughput guarantee.
Native Windows verification remains pending. Use `--contacts 1000 --samples 24
--verify-cache` there and preserve both report and log files.

## Final shared-catalog verification

Final review found another concrete cost: cloning `AppState` deep-copied all 177
contest definitions. The authentication adapter initially added another such
copy; concurrent lightweight requests subsequently hit the benchmark's 30-second
timeout. Authentication now uses a lightweight state, and `ContestRulesStore`
shares its immutable catalog through `Arc`. A pointer-identity regression test
ensures catalog/state clones no longer copy the definitions.

The final optimized Linux run completed 933 requests with a 1,000-contact fixture
and 24 samples per measurement. All 933 request summaries were accounted for:
one coalesced cold contact load/read command, 109 write commands, and exactly one
password verification. All other measured reads and exports used zero worker
commands. Selected p50 / p95 results:

| Workload | Final backend |
| --- | --- |
| Login-disabled `/modes`, sequential | 1.11 / 4.95 ms |
| Login-disabled `/modes`, eight concurrent requests | 8.49 / 11.26 ms |
| Configuration, sequential | 0.58 / 0.79 ms |
| QSO count, concurrent | 3.31 / 3.75 ms |
| Counts alongside writes | 5.04 / 6.79 ms |
| ADIF exports, two concurrent requests | 49.31 / 56.55 ms |
| Serial reservations, concurrent | 7.21 / 8.94 ms |
| Warm-auth QSO commits | 16.30 / 27.52 ms |

These final measurements include catalog sharing as well as database caching;
do not attribute the entire latency improvement to SQLite query elimination.
They remain Linux measurements, not proof of native Windows performance.
