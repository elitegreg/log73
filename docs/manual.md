# Log73 Operator Manual

This manual is for contest operators running the production `log73-backend` executable.

It is written as an operations guide (not a developer guide): start the backend, connect in a browser, configure station/log/radio settings, and operate the logger during a contest.

---

## 1) Introduction: what Log73 is

Log73 is a browser-based amateur radio contest logger with a Rust backend.

In practical terms:

- You run one backend process (`log73-backend`) at the station/site.
- Operators connect to it from one or more browsers.
- The backend stores logs and QSOs in SQLite.
- The backend manages CAT radio control and CW keying.

Core operating capabilities include:

- Multiple logs and radios
- Contest-specific exchange fields loaded from YAML rules
- ADIF and Cabrillo export
- Live CAT mode/frequency updates over websocket
- Run/S&P CW function key messaging (F1..F12)
- ESM-style Enter behavior
- Local pending QSO caching in browser storage for disconnect tolerance

---

## 2) Production deployment layout and startup assumptions

This manual assumes a production-style install similar to:

```text
/opt/log73/bin/log73-backend
~/.config/log73/
~/.local/share/log73/contest-rules/*.yaml
~/.local/share/log73/MASTER.SCP
~/.local/share/log73/cty.csv
~/.local/share/log73/log73.db
```

Recommended production launch pattern:

```bash
/opt/log73/bin/log73-backend --bind 0.0.0.0:7300
```

Important path behavior:

- Config dir defaults to platform config dir (Linux typically `~/.config/log73`).
- Data dir defaults to platform data dir (Linux typically `~/.local/share/log73`).
- Database path defaults to `~/.local/share/log73/log73.db`.
- Contest rules are loaded from both installed data and user data rule directories.

---

## 3) Backend startup + CLI options

Show CLI help:

```bash
/opt/log73/bin/log73-backend --help
```

Current options:

| Option                      | Meaning                                     | Default                                          |
| --------------------------- | ------------------------------------------- | ------------------------------------------------ |
| `--bind <BIND>`             | Bind address/port for browser/API/websocket | `127.0.0.1:7300`                                 |
| `--log-level <LOG_LEVEL>`   | Backend log verbosity                       | `info`                                           |
| `--log-file <LOG_FILE>`     | Optional file log output                    | none                                             |
| `--config-dir <CONFIG_DIR>` | Override config directory                   | platform default                                 |
| `--data-dir <DATA_DIR>`     | Override data directory (includes DB/files) | platform default                                 |
| `--app-dir <APP_DIR>`       | Override app root                           | platform default (`/opt/log73` on Linux layouts) |

Startup assumptions:

- Contest rules must load successfully for backend startup.
- Missing `MASTER.SCP` or `cty.csv` does not necessarily stop startup, but those lookup features degrade.

---

## 4) Connecting with a browser

1. Start backend.
2. Open browser to:
   - `http://<server-ip>:7300/`
3. The UI routes to the Log73 screens.

### Basic Auth behavior (plain language)

If login is enabled in configuration, browser access requires credentials.

- It protects UI + API + websocket endpoints as one shared gate.
- It is not a per-user account system.

### Trust warning

Any trusted operator with current credentials can update credentials in Configure Log73.
Treat it as a shared trusted-station gate, not individual role-based auth.

---

## 5) Configure Log73 screen

Configure Log73 includes:

- Theme selection
- Zoom selection
- Login username/password
- DX Cluster settings
- Logger side image URL (browser-local setting)
- Reset-to-defaults convenience button

### Key details

- Leaving both password fields blank disables login.
- DX Cluster config includes enable toggle, host, port, callsign, max age, and startup commands.
- Logger side image URL is stored in local browser storage (not backend DB) and used by logger UI.
- The logger side image is only shown if download succeeds; refresh attempts occur hourly.

---

## 6) Quick tutorial flow (first-time operation)

1. Open **Configure Log73** and set shared station options.
2. Create a log.
3. Create a radio.
4. Select log + radio in Open Log.
5. Click **Open** to enter logger.
6. Confirm CAT updates and CW key operation before contest start.

---

## 7) Creating/editing logs (with validation details)

### Create log flow

- Choose a contest.
- Enter log name.
- Enter station callsign.
- Fill contest-defined log parameters.

### Validation behavior

- Contest must be known.
- Log name is required (max 100 chars, no control chars).
- Station callsign is required (max 12 chars).
- Contest parameters are validated by contest rule definitions:
  - required fields
  - type constraints
  - regex constraints
  - configured value sets/dropdowns

### Edit behavior

- Contest selection is fixed when editing an existing log.
- Name/callsign/params can be updated (with same validation model).

### Serial allocation context

If a contest uses a sent serial exchange field, serial allocation is managed by backend reservations and client refill behavior. The operator can be blocked from logging if no reserved serial is available.

---

## 8) Creating/editing radios (with validation details)

### Radio fields

- Radio type (`/api/radio-kinds`)
- Name
- Transport: `none`, `tcp`, or `serial` (`none` is for the dummy driver)
- CW tuning increment / SSB tuning increment
- CW keyer type: `none`, `winkeyer`, `cat`, `serial`
- CW messages set (editable and validated)

### Validation behavior

- Radio name required (max 100 chars)
- Radio driver must be supported
- Transport must be `none`, `tcp`, or `serial`
- Non-dummy radios require TCP or serial transport
- Tuning increments must be `1..9999` Hz

Transport-specific:

- TCP mode:
  - host required (max 255 chars, no whitespace/control)
  - port must be `1..65535`
- Serial mode:
  - serial port required (max 255 chars)
  - serial baud must be > 0

CW keyer-specific:

- Winkeyer requires Winkeyer serial port
- Serial CW keyer requires CW serial port
- CW serial line must be `dtr` or `rts`
- If CAT transport is serial and CW keyer is serial **on the same port**, CAT and CW baud rates must match

CW messages:

- Required
- Length-limited
- Control chars restricted
- Validated against CW message parser rules

---

## 9) Opening and using the logger

Logger layout includes:

- Main entry/control window
- Log table window
- Optional band map window
- Optional left-side image panel (download-success only)

Startup behavior:

- Operator callsign prompt appears on logger open.
- You can re-prompt with `Ctrl+O`.

Operational basics:

- Callsign field auto-uppercase and input sanitation
- Live CAT mode/frequency state in title/status areas
- CW function-key banks for Run and S&P
- ESM optional workflows for Enter-driven operation
- Log table supports selection, edit, delete
- `?` button (upper-right) opens help at `/help/index.html`

---

## 10) Offline mode (what works / what doesn’t)

There are two practical offline scenarios.

### A) Backend unreachable before or during navigation

What happens:

- Screens requiring API context cannot fully load data.
- Opening logger reliably requires backend context (log/radio/settings).

What works:

- Very limited navigation shell behavior.

What does not:

- Normal log/radio load, open, and backend-backed operations.

### B) Backend drops while logger is already open

What works:

- You can continue operating in the open logger session.
- New/edited contacts are staged locally with pending/update states.
- Browser local storage persists uncommitted contact queue data.

What degrades/stops:

- Backend commit/refresh actions pause until reconnect.
- Some data-dependent behavior (full backend context/refresh) is delayed.

Reconnect behavior:

- Websocket reconnect attempts use backoff.
- On reconnect, pending operations resume and committed data refresh occurs.

Practical caution:

- If you exit logger while backend is still unavailable, re-entry/open is not reliable until backend returns.

---

## 11) Hotkeys and special input behaviors (complete)

### Global logger shortcuts

| Hotkey           | Action                                                  |
| ---------------- | ------------------------------------------------------- |
| `Ctrl+O`         | Prompt/change operator callsign                         |
| `F1`..`F12`      | Send CW message in active Run/S&P bank                  |
| `Esc`            | Stop CW sending / clear queue (or close CW text dialog) |
| `PageUp`         | CW WPM +1                                               |
| `PageDown`       | CW WPM -1                                               |
| `Ctrl+K`         | Open CW text dialog (CW/CW-R only)                      |
| `Alt+PageUp`     | Shift band up                                           |
| `Alt+PageDown`   | Shift band down                                         |
| `ArrowUp`        | Tune frequency up by configured increment               |
| `ArrowDown`      | Tune frequency down by configured increment             |
| `Ctrl+ArrowDown` | Jump to next band-map spot above VFO                    |
| `Ctrl+ArrowUp`   | Jump to next band-map spot below VFO                    |
| `Alt+M`          | Mark current frequency                                  |
| `Alt+O`          | Store current spot                                      |
| `Alt+Q`          | Jump to last stored CQ frequency                        |
| `Ctrl+P`         | Spot It                                                 |

Notes:

- Function/ESC/Page-style hotkeys are ignored while focus is inside the log table area.
- `Ctrl+K` requires CW/CW-R mode.

### Callsign field Enter behavior

When cursor is in callsign field:

1. Numeric input + Enter -> interpreted as frequency in kHz and sent to radio.
2. Mode token + Enter -> sets radio mode (e.g. `CW`, `CW-R`, `SSB`, `FM`, `FT8`, `RTTY`, etc.).
3. Otherwise Enter participates in ESM/logging logic.

### Field traversal behavior

- `Tab` moves to next empty editable field (custom forward behavior).
- `Shift+Tab` remains normal browser reverse behavior.
- Fixed/read-only fields are skipped for custom forward tabbing.

### CW text dialog (`Ctrl+K`)

| Key                        | Action                                |
| -------------------------- | ------------------------------------- |
| `Space`                    | Send current word with trailing space |
| `Enter`                    | Send current word and close dialog    |
| `Esc`                      | Close dialog                          |
| `Backspace` on empty input | Ignored                               |

---

## 12) ESM behavior summary/matrix

When ESM is enabled, Enter chooses message/log actions from state.

### Run mode

| Condition                                                           | Action                                                                                            |
| ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| Empty callsign                                                      | Send `F1`                                                                                         |
| Callsign present, exchange incomplete (first attempt)               | Send `F5`, then `F2`                                                                              |
| Callsign present, exchange incomplete (repeat attempt)              | Send `F8`                                                                                         |
| Callsign + valid exchange + exchange already sent for that callsign | Send `F3`, then log                                                                               |
| Callsign corrected after sending exchange (CW only)                 | Before `F3`, send corrected suffix if only suffix changed; otherwise send full corrected callsign |

### S&P mode

| Condition                                        | Action              |
| ------------------------------------------------ | ------------------- |
| Empty callsign                                   | Send `F4`           |
| Callsign present, exchange incomplete            | Send `F4`           |
| Callsign + valid exchange, exchange not yet sent | Send `F2`, then log |
| Callsign + valid exchange, exchange already sent | Log directly        |

### Override

- `Alt+Enter` in callsign/exchange fields forces immediate log (normal validation still applies).

---

## 13) Log window selection/editing controls

Selection behavior:

- Click: single select
- `Ctrl+Click` / `Cmd+Click`: toggle row
- `Shift+Click`: range select
- Right-click: context menu (selects row first if needed)

Context menu actions:

- Update selected QSO(s)
- Delete selected QSO(s)

Inline edit keys:

- `Enter`: apply edit
- `Esc`: cancel edit

Other table behavior:

- Infinite-scroll style load-more near bottom (when backend connected and more rows exist)
- Invalid cells are highlighted with validation feedback

---

## 14) Supported contests (names)

Current runtime contest names:

- ARRL Field Day
- CWOps CWT
- K1USN SST
- MST (Medium Speed Test)
- SC QSO Party
- SC QSO Party (In State)

---

## 15) Supported CW keyers

- None
- Winkeyer
- CAT
- Serial (DTR/RTS)

---

## 16) Supported radios

The backend exposes the current `radio-cat-rs` driver descriptors at `/api/radio-kinds`. Each option includes a stable driver `id`, display name, and description.

Common driver IDs include:

- `dummy`
- `elecraft-k4`
- `elecraft-k3`
- `elecraft-k2`
- `kenwood-ts590`
- `kenwood-ts890`
- `kenwood-ts990`
- `icom-ic705`
- `icom-ic7300`
- `icom-ic7610`
- `yaesu-ftdx10`
- `yaesu-ftdx101`
- `yaesu-ft710`
- `yaesu-ft891`
- `yaesu-ft991`

The create-radio screen defaults to the in-memory `dummy` driver with `none` transport.

---

## 17) Core feature checklist

- Browser UI served by backend executable
- Shared Basic Auth gate (optional)
- Contest-rule-driven log setup and exchange validation
- Multi-log create/edit/delete
- Multi-radio create/edit/delete
- Async CAT and websocket radio-state updates
- Run/S&P CW message banks
- ESM Enter workflows
- Local pending queue durability in browser storage
- ADIF export
- Cabrillo export
- Theme/zoom and client-side logger-image preferences
- Embedded help pages at `/help/index.html`

---

## 18) Known limitations

- Basic Auth is a shared station gate, not per-user authorization.
- `Mark`, `Store`, and `Spot It` controls are present but depend on band-map / DX-cluster context; in many configurations they are limited or appear inactive.
- Advanced protocol-family CAT options exist upstream but are not fully exposed in UI fields.
- Full dupe-awareness and backend-derived context can be degraded while disconnected.
- Shared CAT+CW serial-port configurations can still be sensitive to OS/driver behavior even when validation passes.

---

## 19) Practical operating tips

- Before contest start:
  - confirm CAT connectivity and mode/frequency updates
  - confirm CW keying path and WPM control
  - send test function-key messages in both Run and S&P banks
- Keep backend and browser clocks sane (UTC logging consistency matters).
- If backend drops mid-operation, keep logging in the open logger; let pending rows flush after reconnect.
- Avoid exiting logger during outages unless necessary.
- For shared stations, coordinate any login credential changes.
- Rebuild help HTML after manual updates:

```bash
make help
```

---

End of manual.
