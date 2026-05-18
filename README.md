# Log73

Log73 is an amateur radio contest logger prototype. It uses a React/Rsbuild browser frontend and a Rust/Axum backend in a single deployable application.

The current architecture supports multiple logs, multiple radios, browser clients, SQLite storage, and lazy connections to one or more existing `rigctld` instances.

```text
Browser UI
  -> Rust backend
  -> SQLite database
  -> rigctld instances
  -> radios
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
- Per-radio `rigctld` settings:
  - host
  - port
  - poll frequency
  - rigctld communication timeout
- Optional per-radio Winkeyer CW keying settings.
- Run and S&P CW function-key labels/messages.
- Selectable UI themes, persisted in browser local storage.
- Lazy radio connections: a `rigctld` connection opens only when a logger websocket uses that radio.
- Reference-counted radio use: when the last logger websocket for a radio closes, the backend disconnects that radio.
- Per-radio serialized CAT command queue.
- Realtime radio state updates over websocket.
- SQLite-backed QSO storage.
- Offline/pending contact cache in browser local storage.

## Authentication

The app uses HTTP Basic Auth.

Development credentials:

```text
username: log73
password: hamradio
```

Authentication protects the frontend, `/api/*`, and `/ws`.

## Prerequisites

- Node.js / npm
- Rust toolchain
- One or more externally running `rigctld` instances

The backend does **not** start `rigctld`.

Example `rigctld` setup:

```bash
rigctld -m <MODEL> -r <DEVICE> -t 4532
```

For multiple radios, run multiple `rigctld` instances on different ports.

## Quick start: development

Install frontend dependencies:

```bash
npm install
```

Start the backend:

```bash
cd backend
cargo run
```

By default, the backend binds to:

```text
127.0.0.1:7300
```

Use `--bind` to choose a different listen address:

```bash
cargo run -- --bind 0.0.0.0:7300
cargo run -- --bind 127.0.0.1:8080
```

Backend logging defaults to `info` on stdout. You can change the level or write to a file. At `debug` level, incoming request details and pretty-printed POST JSON bodies are logged, with sensitive HTTP headers redacted:

```bash
cargo run -- --log-level debug
cargo run -- --bind 0.0.0.0:7300 --log-level info --log-file log73.log
cargo run -- --contest-rules-dir ../contest-rules
```

Start the frontend dev server in another terminal:

```bash
npm run dev
```

In development, Rsbuild proxies `/api` and `/ws` to the backend on port `7300`.

Open the app, authenticate with the Basic Auth credentials above, then:

1. Open `/ui/open_log`.
2. Create or select a log.
3. Create or select a radio.
4. Open the logger.

## Production build

Build frontend assets:

```bash
npm run build
```

Build the backend:

```bash
cd backend
cargo build --release
```

The backend embeds and serves the built frontend assets from `dist/`.

Run the production backend:

```bash
cd backend
./target/release/log73-backend
```

Production logging options are the same:

```bash
./target/release/log73-backend --bind 127.0.0.1:7300 --log-level debug
./target/release/log73-backend --bind 0.0.0.0:7300 --log-level info --log-file log73.log
```

## Development checks

Frontend:

```bash
npm run lint
npm run build
```

Backend:

```bash
cd backend
cargo fmt
cargo check
cargo run -- --bind 127.0.0.1:7300 --log-level debug
cargo run -- --bind 127.0.0.1:7300 --log-level info --log-file /tmp/log73.log
```

## Project layout

```text
src/                                  React/Rsbuild frontend
src/index.jsx                         frontend entry point
src/app/App.jsx                       frontend routes and theme application
src/screens/OpenLogScreen.jsx         log/radio selection screen and theme picker
src/screens/CreateLogScreen.jsx       create log screen
src/screens/CreateRadioScreen.jsx     create radio screen, rigctld, and Winkeyer settings
src/screens/LoggerScreen.jsx          logger state, websocket, contact commit flow
src/logger/MainWindow.jsx             main logger entry/radio/CW-control UI
src/logger/LogWindow.jsx              QSO table
src/lib/api.js                        API and websocket URL helpers
src/domain/contactFields.js           contact field helpers
src/themes/themes.js                  theme metadata and persistence helpers
src/styles/*.css                      base styles and theme overrides

backend/                              Rust backend
backend/src/main.rs                   Axum routes, websocket handling, API handlers
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

All JSON API routes are under `/api`.

```text
GET    /api/contest-rules
GET    /api/contest-settings?contest_id=<contest_id>

GET    /api/logs
POST   /api/logs
GET    /api/logs/:id
DELETE /api/logs/:id

GET    /api/logs/:log_id/contacts
POST   /api/logs/:log_id/contacts
DELETE /api/contacts/:id

GET    /api/radios
POST   /api/radios
GET    /api/radios/:id
DELETE /api/radios/:id
GET    /api/radios/:id/cw-labels
```

Deletion rules:

- Logs with QSOs cannot be deleted.
- Radios currently used by an active logger websocket cannot be deleted.

## Websocket API

Logger websocket:

```text
/ws?session_id=<uuid>&radio_id=<radio_id>
```

The frontend stores `session_id` in browser local storage and includes it on locally logged contacts as `_session_id`. The backend uses it to avoid echoing the same committed contact back to the originating websocket.

Server radio state message:

```json
{ "type": "radio_state", "frequency_hz": 14025000, "mode": "CW" }
```

Server log/contact messages:

```json
{ "type": "log_entry", "contact": { "_status": "Committed" } }
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
{ "type": "stop_cw" }
{ "type": "set_wpm", "wpm": 25 }
```

## Database

SQLite database file:

```text
backend/log73.db
```

The database is created automatically in the backend working directory.

Tables:

```text
config
logs
radios
qsos
```

Important schema notes:

- `logs` stores log name, contest id, station callsign, and contest parameter JSON.
- `radios` stores rigctld host, port, poll frequency, rigctld timeout, Winkeyer settings, and CW message text.
- `qsos.LOG_ID` references `logs.ID`.
- `idx_qsos_log_id` indexes `qsos(LOG_ID)`.
- Foreign keys are enabled.
- Tables are SQLite `STRICT` tables.

There are no migrations yet. If schema changes during development, remove `backend/log73.db` manually and restart the backend.

## Radio configuration

Each radio row contains:

```text
name
rigctld_host
rigctld_port
poll_frequency
rigctld_timeout
winkeyer_enabled
winkeyer_serial_port
cw_messages
```

Create-radio defaults:

```text
rigctld_host: 127.0.0.1
rigctld_port: 4532 + existing_radio_count
poll_frequency: 0.25
rigctld_timeout: 2
winkeyer_enabled: false
winkeyer_serial_port: ""
cw_messages: built-in default Run/S&P function-key messages
```

`poll_frequency` controls how often the backend polls frequency/mode.

`rigctld_timeout` controls the communication timeout for individual rigctld commands. This should usually be larger than `poll_frequency`; `2` seconds is the default.

Radio connections are lazy. Opening a logger with `radio_id=X` starts or reuses that radio's managed connection. Closing the logger releases it. When the reference count reaches zero, the backend disconnects and removes the managed radio.

Each radio has one async command queue, so CAT commands for that radio are serialized.

## Radio behavior

Radio state is represented as:

```text
frequency_hz
mode
```

`USB` and `LSB` from rigctld are normalized to frontend mode `SSB`.

When the frontend asks for `SSB`, the backend chooses:

- `LSB` on 160m, 80m, and 40m
- `USB` on 20m and shorter bands

Band selection from the logger sends both:

1. set frequency to the selected band's lower edge
2. set mode to the currently selected logger mode

This is intentional because many radios restore a per-band last-used mode when changing bands.

## Contact data

Contacts use ADIF-like JSON fields.

Important fields:

```text
QSO_DATE_TIME_ON  seconds since UTC epoch
STATION_CALLSIGN  selected log's station callsign
OPERATOR          prompted operator callsign
CONTEST_ID        contest id
CALL              worked station
BAND              band name
FREQ              frequency in Hz
MODE              normalized mode
_log_id           database log id
_id               database QSO id
_status           Pending, Updating, or Committed
_session_id       frontend websocket session id
_client_id        temporary frontend id for pending rows
```

Fields mapped to database columns are stored directly in `qsos`. Extra non-private fields are serialized into the `JSON` column. Fields beginning with `_` are private/transient and are not stored in `JSON`.

Committed contacts are loaded from the backend. Pending/updating contacts are cached in browser local storage as an offline/outbox cache.

## Contest rules

Contest rules are loaded from YAML files in `contest-rules/` by default. The backend option `--contest-rules-dir` can point at another directory.

Current SC QSO Party rule IDs:

```text
SC-QSO-PARTY             out-of-state
SC-QSO-PARTY (In State)  in-state
```

Both variants use bands `160, 80, 40, 20, 15, 10, 6, 2` and modes `SSB, FM, AM, CW`.

Log creation dynamically requests required rule parameters:

- `SC-QSO-PARTY`: `State`
- `SC-QSO-PARTY (In State)`: `County`

Those values populate fixed sent exchange fields in the logger. The previous `BERK` default is no longer used.

## UI themes

The open-log screen includes a theme selector. The selected theme is stored in browser local storage under `log73.theme` and applied by adding a theme class to `document.body`.

Available themes:

```text
Default
Modern Dark Radio
Classic Terminal
Clean Light Desktop
N1MM-ish Contest
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
- CW function-key labels are loaded from `/api/radios/:id/cw-labels` for separate Run and S&P banks.
- Run/S&P operating mode chooses which CW function-key bank is active.
- Run F1 can be repeated automatically after CW send completion when repeat is enabled.
- S&P F1 sends the QRL message and then switches to Run mode.
- Stop CW sends a websocket `stop_cw` command.
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
- Backend does not start or supervise `rigctld`.
- No hamlib rig model configuration yet.
- No cluster, band map, SO2R, or multi-transmitter rule enforcement yet.
