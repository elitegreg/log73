# Log73

Log73 is an amateur radio contest logger prototype. It uses a React/Rsbuild browser frontend, a Rust/Axum backend, and a Rust desktop launcher.

The current architecture supports multiple logs, multiple radios, browser clients, SQLite storage, and lazy CAT connections created through `radio-cat-rs`.

```text
Browser UI
  -> Rust backend
  -> SQLite database
  -> radio-cat-rs transports
  -> radios

Desktop launcher UI
  -> starts/stops Rust backend process
```

## Current status

Log73 is under active development. Contest definitions are loaded from YAML rule files. Database migrations are not implemented yet; if the development schema changes, delete the local development database and let the backend recreate it.

## Features

- HTTP Basic Auth for the whole app.
- Structured backend logging with `tracing`, configurable by CLI.
- Browser UI served by the Rust backend in production.
- Separate frontend/backend development mode with Rsbuild proxying `/api` and `/ws`.
- Multi-log selection and creation.
- Multi-radio selection and creation.
- Per-radio CAT settings:
  - radio driver
  - no transport for dummy radios, TCP host and port, or serial port and baud rate
- Optional per-radio Winkeyer CW keying settings.
- Run and S&P CW function-key labels/messages.
- Selectable UI themes, persisted in browser local storage.
- Lazy radio connections: a CAT connection opens only when a logger websocket uses that radio.
- Reference-counted radio use: when the last logger websocket for a radio closes, the backend disconnects that radio.
- Per-radio serialized CAT command queue.
- Realtime radio state updates over websocket.
- SQLite-backed QSO storage.
- Offline/pending contact cache in browser local storage.

## Authentication

The app can use HTTP Basic Auth.

Login is disabled by default: if either login field is blank, the app is accessible without authentication.
Use **Configure** from the main screen to set a username and password.

When login is enabled, authentication protects the frontend, `/api/*`, and `/ws`.

## Prerequisites

- Node.js / npm
- Rust toolchain
- A CAT-capable radio or CAT TCP endpoint supported by `radio-cat-rs`

The backend does **not** start or supervise external CAT daemons.

Current `radio-cat-rs` support is driver/profile-driven and model-dependent. For Elecraft K4 CAT, use driver `elecraft-k4` over either TCP or serial transport. The default create-radio selection is the in-memory `dummy` driver with no transport.

Example TCP CAT target:

```bash
127.0.0.1:5002
```

## Quick start: development

Install frontend dependencies:

```bash
npm install
```

Start the backend from a source checkout, using the repository data directory:

```bash
cargo run -p log73-backend -- --data-dir ./data
```

By default, the backend binds to:

```text
127.0.0.1:7300
```

Use `--bind` to choose a different listen address:

```bash
cargo run -p log73-backend -- --bind 0.0.0.0:7300
cargo run -p log73-backend -- --bind 127.0.0.1:8080
```

Backend logging defaults to `info` on stdout. You can change the level or write to a file. At `debug` level, incoming request details and selected POST JSON payload summaries are logged, with sensitive HTTP headers redacted, sensitive payload fields redacted, and long payloads truncated:

```bash
cargo run -p log73-backend -- --data-dir ./data --log-level debug
cargo run -p log73-backend -- --data-dir ./data --bind 0.0.0.0:7300 --log-level info --log-file log73.log
cargo run -p log73-backend -- --config-dir /tmp/log73-config --data-dir ./data
```

Start the frontend dev server in another terminal:

```bash
npm run dev
```

In development, Rsbuild proxies `/api` and `/ws` to the backend on port `7300`.

Open the app, then:

1. Open `/ui/open_log`.
2. Optionally use **Configure** to enable login.
3. Create or select a log.
4. Create or select a radio.
5. Open the logger.

## Production build

Build frontend assets:

```bash
npm run build
```

Build the backend:

```bash
cargo build --release -p log73-backend
```

The backend embeds and serves the built frontend assets from `dist/`.

Runtime path defaults:

- Linux config: `~/.config/log73/`
- Linux data: `~/.local/share/log73/`
- Linux contest rules: `~/.local/share/log73/contest-rules/`
- Linux application root/app dir: `/opt/log73/` (`bin/log73-backend` under that root)
- macOS and Windows use their platform-specific Log73 config/data/app directories.

Run the production backend from a source checkout:

```bash
./target/release/log73-backend --data-dir ./data
```

Production logging options are the same:

```bash
./target/release/log73-backend --data-dir ./data --bind 127.0.0.1:7300 --log-level debug
./target/release/log73-backend --data-dir ./data --bind 0.0.0.0:7300 --log-level info --log-file log73.log
```

Run the launcher:

```bash
cargo run -p launcher
```

## Release packages

Release packaging is configured with `cargo-dist` in `dist-workspace.toml`.
Tag pushes such as `v0.1.0` run `.github/workflows/release.yml`, building:

- Windows `.msi` installers from cargo-dist
- macOS `.pkg` installers from cargo-dist
- Linux `.deb` and `.rpm` packages with nFPM, using cargo-dist-built binaries
- cargo-dist archive artifacts for all configured targets

The release workflow builds frontend assets first so `log73-backend` embeds the current `dist/` output.

Local package planning:

```bash
cargo install cargo-dist --version 0.32.0 --locked
~/.cargo/bin/dist plan --allow-dirty
```

Linux native packages can be built with:

```bash
make deb
```

This uses `DEB_TARGET=x86_64-unknown-linux-gnu` and the backend crate version by default. Override them when needed:

```bash
make deb DEB_TARGET=aarch64-unknown-linux-gnu VERSION=0.1.0
```

Launcher main screen controls:

- Start/Stop backend process controls with status
- Open log file in the OS default editor/viewer
- Open app in the default browser
- Open app in browser app mode (`--app`) with `1200x800` initial size
- Menu button to open launcher settings

Launcher settings screen controls:

- Backend binary path (editable)
- Config directory, data directory, and app directory
- Bind mode: `localhost only` (`127.0.0.1`) or `open` (`0.0.0.0`)
- Port (default `7300`)
- Log level and log file path
- App-mode browser choice: `chrome` / `chromium` / `edge`
- Set defaults button

Launcher settings are persisted in the Log73 platform config directory. Browser app mode uses a per-browser `--user-data-dir`; by default this is under the Log73 config directory (for example `~/.config/log73/chrome/` on Linux). On Linux, snap-managed browsers use a snap-compatible profile directory under `~/snap/<package>/common/` (for example `~/snap/chromium/common/log73-profile-chromium`). Stop uses graceful termination first (where supported) and falls back to force-stop after a timeout. Backend stdout/stderr are forwarded to the launcher console for debugging startup/runtime errors.

## Development checks

Frontend:

```bash
npm run lint
npm run build
```

Backend:

```bash
cargo fmt --all
cargo check --workspace
cargo run -p log73-backend -- --data-dir ./data --bind 127.0.0.1:7300 --log-level debug
cargo run -p log73-backend -- --data-dir ./data --bind 127.0.0.1:7300 --log-level info --log-file /tmp/log73.log
cargo run -p launcher
```

## Project layout

```text
src/                                  React/Rsbuild frontend
src/index.jsx                         frontend entry point
src/app/App.jsx                       frontend routes and theme application
src/screens/OpenLogScreen.jsx         log/radio selection screen and theme picker
src/screens/CreateLogScreen.jsx       create log screen
src/screens/CreateRadioScreen.jsx     create radio screen, CAT transport, and Winkeyer settings
src/screens/LoggerScreen.jsx          logger state, websocket, contact commit flow
src/logger/MainWindow.jsx             main logger entry/radio/CW-control UI
src/logger/LogWindow.jsx              QSO table
src/lib/api.js                        API and websocket URL helpers
src/domain/contactFields.js           contact field helpers
src/themes/themes.js                  theme metadata and persistence helpers
src/styles/*.css                      base styles and theme overrides

backend/                              Rust backend
backend/src/main.rs                   Axum routes, websocket handling, API handlers
launcher/                             Rust iced desktop launcher
launcher/src/main.rs                  launcher UI and backend process start/stop controls
backend/src/auth.rs                   HTTP Basic Auth middleware
backend/src/db.rs                     SQLite schema and data mapping
backend/src/radio.rs                  radio/CW websocket messages, mode conversion helpers
backend/src/radio_manager.rs          lazy/refcounted multi-radio manager and CW task
backend/src/cw.rs                     CW message parsing, labels, and template rendering
backend/src/static_assets.rs          embedded frontend asset serving
backend/src/scqso_in_state.rs         SC QSO Party contest rules
backend/src/bands.rs                  USA amateur band helpers
backend/src/frequency.rs              frequency type and macros
```

## UI routes

```text
/                         redirects to /ui/open_log
/ui/open_log              select/create/delete logs and radios
/ui/config                configure login credentials
/ui/create_log            create a log
/ui/create_radio          create a radio
/ui/logger/:logId/:radioId logger for selected log and radio
```

Logger context includes:

```text
log_id
radio_id
log name
radio name
contest id
station callsign
operator callsign
```

Operator callsign is contest/QSO metadata, not an authentication identity.

## Backend API

All JSON API routes are under `/api`. The Cabrillo download endpoint also lives under `/api`.

```text
GET    /api/contest-rules
GET    /api/contest-settings?contest_id=<contest_id>

GET    /api/logs
POST   /api/logs
GET    /api/logs/:id
PUT    /api/logs/:id
DELETE /api/logs/:id
GET    /api/logs/:id/qso-count
GET    /api/logs/:id/stats
POST   /api/logs/:id/adif
POST   /api/logs/:id/cabrillo
POST   /api/logs/:id/serial-allocation

GET    /api/logs/:log_id/contacts
POST   /api/logs/:log_id/contacts
DELETE /api/contacts/:id

GET    /api/radios
POST   /api/radios
GET    /api/radio-kinds
GET    /api/radios/:id
DELETE /api/radios/:id
GET    /api/radios/:id/cw-labels
GET    /api/radios/:id/message-labels
GET    /api/radios/cw-messages/default
POST   /api/radios/cw-messages/validate
GET    /api/radios/voice-messages/default
POST   /api/radios/voice-messages/validate
```

Deletion rules:

- Deleting a log also deletes its QSOs.
- Radios currently used by an active logger websocket cannot be deleted.

## Websocket API

Logger websocket:

```text
/ws?session_id=<uuid>&radio_id=<radio_id>
```

The frontend stores `session_id` in browser local storage and includes it on locally logged contacts as `contact.meta.sessionId`. The backend uses it to avoid echoing the same committed contact back to the originating websocket.

Server radio state message:

```json
{ "type": "radio_status", "online": true }
{ "type": "radio_state", "frequency_hz": 14025000, "mode": "CW", "rit_offset_hz": 0 }
```

`radio_status` reports whether CAT/rig control is currently online. It is sent when the websocket starts and whenever the CAT status changes.

Server log/contact messages:

```json
{ "type": "log_entry", "contact": { "meta": { "status": "Committed" }, "adif": {} } }
{ "type": "contact_deleted", "id": 123, "log_id": 1 }
```

Server CW completion message:

```json
{ "type": "cw_sent", "request_id": "uuid-or-client-id" }
```

Client radio commands:

```json
{ "type": "set_frequency", "frequency_hz": 14025000 }
{ "type": "set_mode", "mode": "SSB" }
```

Client CW commands:

```json
{ "type": "send_cw", "request_id": "uuid-or-client-id", "mode": "run", "key": "F1", "fields": { "CALL": "K1ABC" } }
{ "type": "stop_keying" }
{ "type": "set_wpm", "wpm": 25 }
```

## Database

SQLite database file:

```text
<data-dir>/log73.db
```

On Linux, the default data directory is `~/.local/share/log73/`, so the default database path is `~/.local/share/log73/log73.db`. The database is created automatically.

Tables:

```text
config
logs
radios
qsos
```

Important schema notes:

- `logs` stores log name, contest id, station callsign, and contest parameter JSON.
- `radios` stores radio driver, CAT transport settings, keyer settings, sound device ids, and CW/voice message text.
- `qsos.LOG_ID` references `logs.ID`.
- `log_serial_state` stores durable next-serial counters by log id and sent serial ADIF field.
- `idx_qsos_log_id` indexes `qsos(LOG_ID)`.
- Foreign keys are enabled.
- Tables are SQLite `STRICT` tables.

There are no migrations yet. If schema changes during development, remove `log73.db` from the active data directory manually and restart the backend.

## Radio configuration

Each radio row contains:

```text
name
radio_kind
transport_kind
tcp_host
tcp_port
serial_port
serial_baud_rate
options
cw_tuning_increment_hz
ssb_tuning_increment_hz
rit_clear_on_log
voice_input_device_id
voice_output_device_id
cw_keyer_type
winkeyer_serial_port
cw_serial_port
cw_serial_baud_rate
cw_serial_line
cw_messages
voice_messages
```

Create-radio defaults:

```text
radio_kind: dummy
transport_kind: none
tcp_host: 127.0.0.1
tcp_port: 5002
serial_port: ""
serial_baud_rate: 115200
options: ""
cw_tuning_increment_hz: 20
ssb_tuning_increment_hz: 100
rit_clear_on_log: false
voice_input_device_id: null
voice_output_device_id: null
cw_keyer_type: none
winkeyer_serial_port: ""
cw_serial_port: ""
cw_serial_baud_rate: 9600
cw_serial_line: dtr
cw_messages: built-in default Run/S&P CW function-key messages
voice_messages: built-in default Run/S&P voice function-key messages
```

Voice messages use the same Run/S&P F-key text format as CW messages. The value after the comma is a WAV path relative to `<data-dir>/voicekeyer/` (for example `operator1/CQ.wav`), or an action token such as `{Action:Clear}`. Voice file paths may include message-field placeholders such as `{OPERATOR}` and `{STATION_CALLSIGN}`; those are resolved when the message is sent. Empty voice-message file values unregister that radio/mode F-key.

Radio connections are lazy. Opening a logger with `radio_id=X` starts or reuses that radio's managed connection. Closing the logger releases it. When the reference count reaches zero, the backend disconnects and removes the managed radio.

Each radio has one async command queue, so CAT commands for that radio are serialized.
If CAT is offline, reconnect attempts back off exponentially from `1s` to a `10s` maximum.

## Radio behavior

Radio state is represented as:

```text
frequency_hz
mode
rit_offset_hz
```

`USB` and `LSB` from `radio-cat-rs` are normalized to frontend mode `SSB`.

When the frontend asks for `SSB`, the backend chooses:

- `LSB` on 160m, 80m, and 40m
- `USB` on 20m and shorter bands

Band selection from the logger sends both:

1. set frequency to the selected band's lower edge
2. set mode to the currently selected logger mode

This is intentional because many radios restore a per-band last-used mode when changing bands.

## Contact data

Contacts now have two maps:

```text
meta  transient/state/identity fields
adif  logged QSO fields
```

Important `adif` fields:

```text
QSO_DATE_TIME_ON  seconds since UTC epoch
STATION_CALLSIGN  selected log's station callsign
OPERATOR          prompted operator callsign
CONTEST_ID        contest id
CALL              worked station
BAND              band name
FREQ              frequency in Hz
MODE              normalized mode
```

Important `meta` fields:

```text
logId      database log id
id         database QSO id
status     Pending, Updating, Failed, or Committed
sessionId  frontend websocket session id
clientId   temporary frontend id for pending rows
force      validation override flag
pts        scored QSO points
mult       scored multipliers credited by this QSO
bonus      scored bonus points credited by this QSO
dupe       whether the QSO is currently a dupe
```

Fields mapped to database columns are stored directly from `contact.adif` into `qsos`. Extra ADIF fields are serialized into the `JSON` column. `contact.meta` is transient and is not stored in the QSO JSON payload.

Committed contacts are loaded from the backend. Pending/updating contacts are cached in browser local storage as an offline/outbox cache.

## Contest rules

Contest rules are loaded from YAML files in `<data-dir>/contest-rules/` by default. In a source checkout, run the backend with `--data-dir ./data` to use `data/contest-rules/`.
Scoring-related YAML settings live under a `scoring` block (`qso_points`, `dupe_key`, `multipliers`, `bonus_points`).
Contest-specific Cabrillo metadata lives under a `cabrillo` block (`fixed_fields`, `log_fields`, `export_fields`).
ADIF export uses committed QSO data from the database and derives `QSO_DATE` and `TIME_ON` from the stored `QSO_DATE_TIME_ON` epoch.

Current contest rule IDs include:

```text
ARRL-FIELD-DAY           ARRL Field Day
CWT                      CWOps CWT
K1USNSST                 K1USN SST
MST                      MST (Medium Speed Test)
SC-QSO-PARTY             SC QSO Party out-of-state
SC-QSO-PARTY (In State)  SC QSO Party in-state
```

Log creation dynamically requests required rule parameters where needed:

- `ARRL-FIELD-DAY`: `Class`, `Section`
- `CWT`: `NAME`, `EXCHANGE`
- `K1USNSST`: `NAME`, `QTH`
- `MST`: `SERIAL_BATCH_SIZE`
- `SC-QSO-PARTY`: `State`
- `SC-QSO-PARTY (In State)`: `County`

Those values populate fixed sent exchange fields in the logger. Contests with a sent `Serial` exchange field also get a `SERIAL_BATCH_SIZE` parameter, defaulting to 10; the backend reserves durable serial ranges by log id and field, the browser refills after 90% of the batch is consumed, and the logger blocks logging if no reserved serial is available. The previous `BERK` default is no longer used. The SC QSO Party rules also define Cabrillo category fields at log-create/edit time and additional export-time fields for Cabrillo download.
For `SC-QSO-PARTY (In State)`, the received value is labeled `Exchange` because it may be a county, state/province, or `DX`.

## UI themes

The open-log screen includes a theme selector. The selected theme is stored in browser local storage under `log73.theme` and applied by adding a theme class to `document.body`.

Available themes:

```text
Default
Modern Dark Radio
Classic Terminal
Clean Light Desktop
High Contrast
```

## Logger UI behavior

- Station callsign comes from the selected log.
- Operator callsign is prompted when opening the logger and can be changed with Ctrl+O.
- Callsigns are uppercased and limited to 12 characters.
- If the callsign field contains a number and Enter is pressed, it is treated as a frequency in kHz and sent to the radio.
- Radio mode/frequency come from backend websocket radio state.
- The server indicator shows websocket connection status.
- The title bar shows log, radio, contest, mode, and frequency.
- CW WPM is stored in browser local storage under `log73.cw_wpm` and sent to the backend when the websocket is connected.
- Function-key labels are loaded from `/api/radios/:id/message-labels` for separate CW/voice and Run/S&P banks.
- Run/S&P operating mode chooses which function-key bank is active; radio mode chooses CW messages or voice messages.
- Run F1 can be repeated automatically after CW or voice-keyer completion when repeat is enabled.
- S&P F1 sends the QRL message and then switches to Run mode.
- Stop Sending sends a websocket `stop_keying` command to stop CW or voice keying.
- Exit Logger returns to the log/radio selection screen.
- Fixed exchange fields are read-only and skipped during tab navigation.
- RST validation depends on mode:
  - CW: three digits, `111` through `599`
  - non-CW: two digits, `11` through `59`
- The QSO table is sorted newest first.
- Pending/uncommitted contacts are highlighted until committed by the backend.

## Limitations / future work

- Contest scoring and validation are still incomplete; YAML metadata is loaded for future validation work.
- Basic Auth credentials are static development credentials.
- No database migrations yet.
- Backend does not start or supervise external CAT daemons.
- Radio support is limited by the `radio-cat-rs` factory model list (see `/api/radio-kinds`).
- No cluster, band map, SO2R, or multi-transmitter rule enforcement yet.
