# log73

log73 is an amateur radio contest logger prototype. It is designed as a client/server application with a JavaScript/React frontend and a Rust backend.

The current version demonstrates dynamic contest configuration loading. The backend serves contest rules for the SC QSO Party in-state module, and the frontend uses those rules to build exchange entry fields and the QSO table.

## Project layout

- `src/` - React/Rsbuild frontend
- `backend/` - Rust HTTP backend
- `backend/src/frequency.rs` - frequency type and `khz!` / `mhz!` macros
- `backend/src/bands.rs` - USA amateur band definitions and band lookup helpers
- `backend/src/scqso_in_state.rs` - SC QSO Party in-state contest rules

## Backend

The backend runs on `http://127.0.0.1:8080` and currently provides static JSON endpoints:

- `GET /contest-settings/get` - contest name, allowed bands, allowed modes, exchange field definitions, and QSO table columns
- `GET /contacts/get` - currently returns an empty contact list

Run it with:

```bash
cd backend
cargo run
```

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

The frontend expects the backend to be running on port `8080` at the same hostname as the frontend, for example `http://127.0.0.1:8080`. On startup it prompts for an operator callsign, uppercases it, then loads contest settings and contacts. If loading fails, it shows an alert.

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
- Radio mode is currently static: `CW`.
- Radio frequency is currently static: `14025`.
- Title bar format is `Mode: RADIO_MODE, Freq: RADIO_FREQ - CONTEST_NAME`.
- Callsigns are uppercased and limited to 12 characters.
- Operator callsign is prompted on startup, uppercased, and shown in the status line.
- Exchange fields are rendered from backend contest settings.
- Fixed exchange fields are prefilled, read-only, and skipped during tab navigation.
- The last editable exchange field tabs back to the callsign field.
- `RST` fields accept only digits `1` through `9`; the first digit must be `1` through `5`.
- In CW mode, `RST` fields are three characters, from `111` through `599`.
- In non-CW modes, `RST` fields are two characters, from `11` through `59`; out-of-range defaults such as `599` are trimmed.
- The status line shows `STATION_CALLSIGN / Op: OPERATOR_CALLSIGN`.
- The QSO table columns are rendered from backend `qso_columns`.
