# log73

log73 is an amateur radio contest logger prototype. It is designed as a client/server application with a JavaScript/React frontend and a Rust backend.

The current version demonstrates dynamic contest configuration loading and basic radio control. The backend serves contest rules for the SC QSO Party in-state module, connects to an existing `rigctld` instance, polls radio frequency/mode, and publishes realtime state to the frontend over the backend websocket. The frontend uses contest rules to build exchange entry fields and the QSO table, while the title bar and mode controls reflect the live radio state.

## Project layout

- `src/` - React/Rsbuild frontend
- `backend/` - Rust HTTP backend
- `backend/src/frequency.rs` - frequency type and `khz!` / `mhz!` macros
- `backend/src/bands.rs` - USA amateur band definitions and band lookup helpers
- `backend/src/scqso_in_state.rs` - SC QSO Party in-state contest rules

## Backend

The backend runs on `http://127.0.0.1:8080`, provides contest/contact JSON endpoints, and exposes a backend websocket:

- `GET /contest-settings/get` - contest name, allowed bands, allowed modes, exchange field definitions, and QSO table columns
- `GET /contacts/get` - currently returns an empty contact list
- `GET /ws` - backend websocket for realtime updates and commands, including radio state updates and radio set commands

Run it with:

```bash
cd backend
cargo run
```

By default the backend connects to an already-running `rigctld` at `127.0.0.1:4532` and polls every `0.25` seconds. It does not start `rigctld` itself. Runtime options:

```bash
cd backend
cargo run -- \
  --rigctld-host 127.0.0.1 \
  --rigctld-port 4532 \
  --poll-frequency 0.25
```

Radio state is modeled in the backend as frequency in Hz plus a normalized mode string. `USB` and `LSB` from rigctld are published to the frontend as `SSB`. Frontend `SSB` set commands are converted back to `LSB` on 160m through 40m and `USB` on 20m and shorter bands.

The backend websocket sends server messages like:

```json
{ "type": "radio_state", "frequency_hz": 14025000, "mode": "CW" }
```

The frontend connects to the backend websocket with a session id query parameter, for example `/ws?session_id=<uuid>`. The session id is stored in browser local storage and is included on locally logged contacts as `_session_id`.

The frontend sends radio commands like:

```json
{ "type": "set_frequency", "frequency_hz": 14025000 }
{ "type": "set_mode", "mode": "SSB" }
```

When a contact is posted to `POST /contacts`, the backend marks it with `_status: "Committed"`, returns the committed contact, and broadcasts it to other backend websocket sessions as:

```json
{ "type": "log_entry", "contact": { "_session_id": "...", "_status": "Committed" } }
```

The backend does not echo a `log_entry` to websocket clients with the same session id as the posted contact. Clients that receive a `log_entry` from another session add it to their contacts list.

Useful backend checks:

```bash
cd backend
cargo fmt
cargo check
```

## Frontend

Install dependencies:

```bash
pnpm install
```

Start the dev server:

```bash
pnpm run dev
```

The frontend expects the backend to be running on port `8080` at the same hostname as the frontend, for example `http://127.0.0.1:8080`. On startup it prompts for an operator callsign, uppercases it, opens `/ws` for radio state, then loads contest settings and contacts. If loading fails, it shows an alert.

Build for production:

```bash
pnpm run build
```

Useful frontend checks:

```bash
pnpm run lint
pnpm run build
```

## Current contest module

The active contest is `SC-QSO-PARTY`.

Configured allowed bands, in meters:

- 160
- 80
- 40
- 20
- 15
- 10
- 6
- 2

Configured allowed modes:

- SSB
- FM
- AM
- CW

Exchange fields:

- `RST(s)` - type `RST`, ADIF `RST_SENT`, default `599`
- `County` - type `String:4`, ADIF `STX_STRING`, default `BERK`, fixed
- `RST(r)` - type `RST`, ADIF `RST_RCVD`
- `State` - type `String:4`, ADIF `SRX_STRING`

QSO table columns:

- `Date`
- `Time`
- `Freq`
- `Mode`
- `Call`
- `RST(s)`
- `RST(r)`
- `Mult`
- `Pts`
- `Op`

## Current UI behavior

- Station callsign is currently static: `NG4M`.
- Radio mode and frequency come from radio state messages on the backend websocket, with fallback defaults of `CW` and `14025` kHz before the first update.
- Title bar format is `Mode: RADIO_MODE, Freq: RADIO_FREQ - CONTEST_NAME`.
- The old text menu has been replaced with radio controls. `Band` lists the contest's allowed bands and follows the radio's current band; if the radio is on a non-contest or unknown band, the Band control is shown in red. Selecting a band tunes to that band's lower edge. `Mode` offers `CW`, `SSB`, `FM`, and `AM`; selecting one sends a websocket command to the backend.
- Callsigns are uppercased and limited to 12 characters.
- If the callsign field contains only a number or decimal number and Enter is pressed, it is treated as a frequency in kHz, converted to Hz, sent to the backend, and the field is cleared. For example, `14025` sends `14025000` Hz and `14025.5` sends `14025500` Hz.
- Operator callsign is prompted on startup, uppercased, and shown in the status line.
- Exchange fields are rendered from backend contest settings.
- Fixed exchange fields are prefilled, read-only, and skipped during tab navigation.
- The last editable exchange field tabs back to the callsign field.
- `RST` fields accept only digits `1` through `9`; the first digit must be `1` through `5`.
- In CW mode, `RST` fields are three characters, from `111` through `599`.
- In non-CW modes, `RST` fields are two characters, from `11` through `59`; out-of-range defaults such as `599` are trimmed.
- The status line shows `STATION_CALLSIGN / Op: OPERATOR_CALLSIGN`.
- The QSO table columns are rendered from backend `qso_columns`.
- The log table is sorted with the most recent contact first.
