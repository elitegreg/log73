# Log73 Operator Manual

This manual is for contest operators running the production `log73-backend` executable.

It is written as a practical guide: start the backend, connect from a browser, configure it, create logs/radios, and operate the logger.

---

## 1) What Log73 is

Log73 is a browser-based amateur radio contest logger with a Rust backend.

In plain terms:

- You run **one backend program** (`log73-backend`) on a computer in your shack/site.
- You connect to it from one or more web browsers.
- The backend stores logs/QSOs in SQLite.
- The backend talks to radios through CAT (`radio-cat-rs`) and can key CW using CAT, Winkeyer, or serial DTR/RTS.

Log73 supports:

- Multiple logs
- Multiple radios
- Contest-specific exchange fields
- ADIF export
- Cabrillo export
- Live CAT frequency/mode updates
- CW function keys (Run and S&P banks)
- ESM-style Enter behavior
- Offline/pending QSO caching in browser storage

---

## 2) Production deployment layout (assumed)

This manual assumes your files are installed like this:

```text
/opt/log73/bin/log73-backend
~/.config/log73/
~/.local/share/log73/contest-rules/*.yaml
~/.local/share/log73/MASTER.SCP
~/.local/share/log73/cty.dat
```

Recommended run pattern:

```bash
/opt/log73/bin/log73-backend \
  --bind 0.0.0.0:7300
```

### Important: where the database is created

`log73-backend` creates/uses `log73.db` in the active data directory.

On Linux, the default database path is:

```text
~/.local/share/log73/log73.db
```

---

## 3) Backend startup and command-line options

Run help:

```bash
/opt/log73/bin/log73-backend --help
```

Current options:

| Option | Meaning | Default |
|---|---|---|
| `--bind <ADDR:PORT>` | Listen address for browser/API/WebSocket clients | `127.0.0.1:7300` |
| `--log-level <LEVEL>` | Backend log verbosity | `info` |
| `--log-file <PATH>` | Append logs to file instead of stdout | (none) |
| `--config-dir <PATH>` | Config directory | platform-specific Log73 config dir |
| `--data-dir <PATH>` | Data directory for `log73.db`, `MASTER.SCP`, `cty.dat`, and `contest-rules/` | platform-specific Log73 data dir |
| `--app-dir <PATH>` | Application install/root directory | platform-specific Log73 app dir (`/opt/log73` on Linux) |
| `-h`, `--help` | Show help | n/a |
| `-V`, `--version` | Show version | n/a |

### Production examples

Bind to all interfaces on port 7300:

```bash
/opt/log73/bin/log73-backend \
  --bind 0.0.0.0:7300
```

Write logs to a file:

```bash
/opt/log73/bin/log73-backend \
  --bind 0.0.0.0:7300 \
  --log-level info \
  --log-file /var/log/log73.log
```

### Startup requirements

- Contest rules directory must load successfully or backend startup fails.
- Data directory files are recommended:
  - `MASTER.SCP` (super check partial suggestions)
  - `cty.dat` (DXCC lookup hints)

If data files are missing, backend still runs, but those lookup features are unavailable.

---

## 4) Connect from a browser

1. Start backend.
2. Open browser to:
   - `http://<server-ip>:7300/`
3. You will be redirected to the Open Log screen.

### If Basic Auth is enabled

When login is enabled, your browser will prompt for username/password before opening the app.

- This protects UI + API + WebSocket endpoints.
- It is **not per-operator account management**. It is a shared gate.

### Basic trust model (important)

Any operator with access to Log73 and current credentials can change credentials in **Configure Log73**.

So treat it as a trusted shared environment:

- Share only with trusted operators.
- Coordinate credential changes.

---

## 5) Quick operating walkthrough (tutorial)

This is the normal sequence on first use.

### Step A: Open "Configure Log73" (optional but recommended)

From Open Log screen, click **Configure Log73**.

You can set:

- Theme
- Zoom
- Login username/password (Basic Auth)

Then Save and return.

### Step B: Create your log

From Open Log:

1. In **Logs**, click **Create**.
2. Select contest.
3. Enter log name + station callsign.
4. Fill required contest parameters.
5. Click **Create**.

### Step C: Create your radio

From Open Log:

1. In **Radios**, click **Create**.
2. Select radio type.
3. Set CAT transport (TCP or serial).
4. Set poll/timeout.
5. Choose CW keying method.
6. Click **Create**.

### Step D: Open the logger

1. Select a log and a radio.
2. Click **Open**.
3. Enter operator callsign when prompted.
4. Start logging.

---

## 6) The "Configure Log73" screen

This screen controls:

- Theme
- Zoom
- Login credentials (Basic Auth)

### Basic Auth behavior

- Login is enabled only when credentials are set.
- If login is disabled, app is open with no password prompt.
- Leaving password blank disables authentication.

### Operational note

Because this is shared Basic Auth, not separate user accounts, every trusted operator effectively has admin-level ability to change credentials.

---

## 7) Creating and editing logs

Use Open Log screen:

- **Create** -> `/ui/create_log`
- **Edit** -> `/ui/edit_log/:logId`

### Fields and validation

#### Common fields

- **Contest**
  - Required when creating
  - Cannot be changed in edit mode
- **Name**
  - Required
  - Max 100 chars
- **Station Callsign**
  - Required
  - Max 12 chars
  - Must contain letters and numbers
  - Allowed characters: letters, numbers, `/`

#### Contest-specific required parameters (current contests)

- **ARRL Field Day**
  - `Class` required (e.g. `1A`, `2B`)
  - `Section` required (valid ARRL section list)
- **CWOps CWT**
  - `First Name` required
  - `Member Number/SPC/DX/CWA` required
- **K1USN SST**
  - `Operator Name` required
  - `State/Province/DX` required
- **MST (Medium Speed Test)**
  - `Serial Batch Size` required, default `10`; refill starts after 90% of the batch is consumed
- **SC QSO Party**
  - `State` required (state/province list or `DX`)
- **SC QSO Party (In State)**
  - `County` required (SC county code list)

#### Cabrillo category fields in log create/edit

Some contests define additional Cabrillo category fields on this screen (for example category mode/operator/station/transmitter). These are validated by contest rules.

### Delete behavior

- Deleting a log deletes its QSOs.
- If a log has QSOs, UI asks for confirmation.

---

## 8) Creating and editing radios

Use Open Log screen:

- **Create** -> `/ui/create_radio`
- **Edit** -> `/ui/edit_radio/:radioId`

### Radio fields

- Radio Type (from supported list)
- Name
- Transport: `TCP` or `Serial`
- TCP host/port **or** serial port/baud
- Poll frequency (seconds)
- CAT timeout (seconds)
- CW keying:
  - None
  - Winkeyer
  - CAT
  - Serial (DTR/RTS)

### Validation and limits

- Name required, max 100 chars
- Radio type must be in supported list
- Transport must be `tcp` or `serial`
- Poll frequency and CAT timeout: `0.01` to `3600` seconds
- TCP mode:
  - host required, max 255 chars
  - port 1..65535
- Serial mode:
  - serial port required, max 255 chars
  - serial baud rate > 0
- CW keyer type must be one of: `none`, `winkeyer`, `cat`, `serial`
- Winkeyer mode: Winkeyer serial port required
- Serial CW mode:
  - CW serial port required
  - CW serial baud rate > 0
  - control line must be `dtr` or `rts`

### CAT serial + CW serial on same port

Log73 allows configuring CAT serial and CW serial to the **same** serial device, with this requirement:

- If same port is used, CAT baud and CW baud must match.

Operationally, this is intended sharing behavior; if your platform/driver cannot support your chosen setup reliably, switch to CAT CW keying or a separate Winkeyer device.

### Edit/delete behavior

- You can edit radios.
- If a radio is active in a running logger session, backend blocks delete.
- Editing an active radio triggers backend reload of that radio config.

---

## 9) Opening the logger and operating basics

From Open Log screen:

1. Select a log.
2. Select a radio.
3. Click **Open**.

If either log or radio is not selected, logger cannot open.

### On entry

- You are prompted for **operator callsign**.
- Default prompt value starts from station callsign.
- You can change later with `Ctrl+O`.

### Main logger UI behaviors

- Run mode selector: `S&P` or `Run`
- Band selector
- Mode selector
- ESM toggle
- CW WPM control (when in CW/CW-R)
- CAT status light
- Server/WebSocket status light
- Callsign + exchange fields
- CW function keys F1..F12 (Run/S&P banks)
- Stop/Wipe/Log It/Rescore/etc buttons

### Contest-dependent exchange fields

- Fields come from contest rules.
- Fixed "sent" fields are read-only and skipped in tab order.
- Validation is contest-specific.

### Dupe alerts

The callsign area can show:

- `Dupe`
- `Possible Dupe`

When backend connectivity is lost, reliable/full dupe checking is no longer guaranteed.

---

## 10) Offline mode (what works, what does not)

This section covers both offline situations:

1. Backend unreachable from browser
2. Backend/WebSocket drops while logger is already open

### What still works while logger stays open

- You can continue entering QSOs.
- New/edited unsaved QSOs are kept locally in browser storage.
- Pending rows are highlighted as uncommitted.
- Data survives browser restart (same browser/profile, as long as site data is not cleared).

### What stops or degrades

- CAT live updates stop (frequency/mode stops updating from radio).
- CAT control commands stop (set freq/mode from UI/typing will not reach radio).
- CW sending stops (function keys, CW text, stop CW, set WPM commands require live WebSocket).
- Reliable/full dupe checking is unavailable (only locally available data can be used).
- Contacts cannot be committed to backend until connection returns.

### Important navigation limitation

If backend is down:

- You may continue inside an already-open logger session.
- But if you **exit** logger, you cannot reliably return/open again until backend is reachable.

### Pending, updating, failed contact states

- **Pending**: new local QSO waiting to upload
- **Updating**: edit to an existing QSO waiting to upload
- **Failed**: backend rejected upload (validation/server response)

Behavior:

- Network outage -> retries continue automatically when possible.
- Backend rejection -> row marked failed; manual correction is needed.

---

## 11) Hotkeys and special keyboard behavior (full reference)

### Global logger hotkeys

| Hotkey | Action |
|---|---|
| `Ctrl+O` | Prompt/change operator callsign |
| `F1`..`F12` | Send CW message button for active Run/S&P bank |
| `Esc` | Stop CW sending/clear queue (or close CW text dialog) |
| `PageUp` | CW WPM +1 |
| `PageDown` | CW WPM -1 |
| `Ctrl+K` | Open CW text dialog (CW/CW-R only) |

> Note: F-key/Esc/Page hotkeys are ignored while focus is inside the log table area.

### Callsign field Enter behavior (special)

When cursor is in callsign field:

1. **Numeric input + Enter** -> treated as frequency in **kHz** and sent to radio.
   - Example: `14074` -> 14.074 MHz
2. **Mode token + Enter** -> sets radio mode.
   - Valid tokens: `CW`, `CW-R`, `SSB`, `FM`, `FT8`, `JT65`, `JT9`, `MFSK`, `PSK`, `RTTY`
3. Otherwise Enter participates in ESM/logging behavior.

### Field navigation behavior

- `Tab` moves to next **empty editable** field (custom behavior).
- Fixed/read-only exchange fields are skipped.
- `Shift+Tab` uses normal browser reverse tab behavior.

### CW text dialog keys (`Ctrl+K`)

| Key | Action |
|---|---|
| `Space` | Send current word with trailing space |
| `Enter` | Send current word and close dialog |
| `Esc` | Close dialog |
| `Backspace` on empty word | Ignored (prevents deleting prior committed text) |

---

## 12) ESM Enter behavior (operator view)

When ESM is enabled, Enter sends context-dependent function keys and may log automatically.

In exchange fields, Enter can also jump to the next incomplete/invalid exchange field before final log actions.

### Run mode summary

- Empty callsign -> sends `F1` (CQ)
- Callsign present, exchange not complete:
  - First attempt: sends `F5` then `F2`
  - Repeat attempt on same call: sends `F8`
- Callsign + valid exchange, exchange already sent for that callsign -> sends `F3` and logs QSO

### S&P mode summary

- Empty callsign -> sends `F4`
- Callsign present, exchange incomplete -> sends `F4`
- Callsign + valid exchange:
  - If exchange not yet sent: sends `F2` and logs
  - If already sent: logs directly

### Alt+Enter override

- `Alt+Enter` in callsign/exchange fields logs immediately (normal validation still applies).

---

## 13) CW function key behavior details

- Separate Run and S&P label/message banks (F1..F12)
- In S&P mode, sending `F1` switches operating mode to Run
- Run `F1` can be auto-repeat when `Rpt` checkbox is enabled
  - Repeat continues only while callsign field is empty
- `Stop Sending` button or `Esc` stops queued/active sending
- CW controls are meaningful in CW/CW-R operation

---

## 14) Log window editing, selection, and context menu

The right-hand log table supports multi-select and inline editing.

### Selection controls

- Click: single select
- `Ctrl+Click` (or `Cmd+Click` on macOS): toggle row in selection
- `Shift+Click`: select range from last selected row
- Right-click row: open context menu (and select row first if not selected)

### Context menu actions

- **Update selected QSO(s)**
- **Delete selected QSO(s)**

### Inline update keyboard controls

- `Enter`: apply update
- `Esc`: cancel update

### Editable columns and parsing rules

- `Date/Time (UTC)` expects `YYYY-MM-DD HH:MM:SS`
- `Freq` expects kHz value; converted internally to Hz
- `Mode` must be one of allowed contest modes
- Exchange fields are validated by contest field definitions
- Sent serial-number exchange columns are read-only
- `Mult` and `Pts` are read-only

---

## 15) Supported contests (current)

- **ARRL Field Day**
- **CWOps CWT**
- **K1USN SST**
- **MST (Medium Speed Test)**
- **SC QSO Party**
- **SC QSO Party (In State)**

---

## 16) Supported CW keyers

- **None** (CW sending disabled)
- **Winkeyer** (external Winkeyer serial device)
- **CAT** (radio CAT CW commands)
- **Serial (DTR/RTS)** (serial control-line keying)

---

## 17) Supported radios (current build)

Current runtime-exposed radio list: **128** kinds.

Advanced protocol options are backend-level and are not currently user-configurable from Log73 screens [4].

### Kenwood + Elecraft (Kenwood CAT family) (30)
- Kenwood TS-140S
- Kenwood TS-680S
- Kenwood TS-711
- Kenwood TS-790
- Kenwood TS-811
- Kenwood TS-690S
- Kenwood TS-50S
- Kenwood TS-930
- Kenwood TS-940S
- Kenwood TS-950S
- Kenwood TS-950SDX
- Kenwood TS-440S
- Kenwood TS-450S
- Kenwood TS-850
- Kenwood TS-870S
- Kenwood TS-570S
- Kenwood TS-570D
- Kenwood TS-2000
- Kenwood TS-480
- Kenwood TS-590S
- Kenwood TS-590SG
- Kenwood TS-890S
- Kenwood TS-990S
- Kenwood TRC-80
- Elecraft K2
- Elecraft K3
- Elecraft K3S
- Elecraft K4
- Elecraft KX3
- Elecraft KX2

### Kenwood-family special profiles (17)
- SDRConsole
- TruSDX
- QRPLabs QCX
- QRPLabs QDX
- QRPLabs QMX
- Hilberling PT-8000A
- SDRPlay SDRUno
- BG2FX FX-4
- BG2FX FX-4C
- BG2FX FX-4CR
- BG2FX FX-4L
- FlexRadio FLEX-6XXX (KENWOOD COMPAT.)
- PowerSDR
- Thetis
- PiHPSDR
- HamGeek USDX
- Lab599 TX-500

### Icom (CI-V) (54) [1]
- Icom IC-707
- Icom IC-725
- Icom IC-726
- Icom IC-728
- Icom IC-729
- Icom IC-735
- Icom IC-736
- Icom IC-737
- Icom IC-738
- Icom IC-751
- Icom IC-761
- Icom IC-765
- Icom IC-775
- Icom IC-781
- Icom IC-271
- Icom IC-275
- Icom IC-375
- Icom IC-471
- Icom IC-475
- Icom IC-575
- Icom IC-820H
- Icom IC-821H
- Icom IC-970
- Icom IC-1275
- Icom IC-706
- Icom IC-706MKII
- Icom IC-706MKIIG
- Icom IC-78
- Icom IC-703
- Icom IC-718
- Icom IC-746
- Icom IC-746PRO
- Icom IC-756
- Icom IC-756PRO
- Icom IC-756PROII
- Icom IC-756PROIII
- Icom IC-7000
- Icom IC-7200
- Icom IC-7410
- Icom IC-910
- Icom IC-9100
- Icom IC-7100
- Icom IC-7600
- Icom IC-7700
- Icom IC-7800
- Icom IC-7300
- Icom IC-7300MK2
- Icom IC-705
- Icom IC-7610
- Icom IC-7760
- Icom IC-7850
- Icom IC-7851
- Icom IC-905
- Icom IC-9700

### Xiegu (CI-V compatible) (5) [1]
- Xiegu X108G
- Xiegu X6100
- Xiegu X6200
- Xiegu G90
- Xiegu X5105

### Yaesu (New CAT) (14) [2]
- Yaesu FT-450
- Yaesu FT-950
- Yaesu FT-2000
- Yaesu FTDX-1200
- Yaesu FTDX-3000
- Yaesu FTDX-5000
- Yaesu FTDX-9000
- Yaesu FTDX-9000-OLD
- Yaesu FT-991
- Yaesu FT-891
- Yaesu FT-710
- Yaesu FTDX-10
- Yaesu FTDX-101D
- Yaesu FTDX-101MP

### FlexRadio SmartSDR native slices (8) [3]
- FlexRadio SMARTSDR-SLICE-A
- FlexRadio SMARTSDR-SLICE-B
- FlexRadio SMARTSDR-SLICE-C
- FlexRadio SMARTSDR-SLICE-D
- FlexRadio SMARTSDR-SLICE-E
- FlexRadio SMARTSDR-SLICE-F
- FlexRadio SMARTSDR-SLICE-G
- FlexRadio SMARTSDR-SLICE-H

#### Radio option footnotes

[1] `radio-cat-rs` supports advanced CI-V options (for custom integrations), e.g. `civ.rig_addr`, `civ.controller_addr`, `civ.retry_max`, `civ.retry_backoff_ms`.

[2] `radio-cat-rs` supports Yaesu options such as `yaesu.retry_max`, `yaesu.retry_backoff_ms`, and `yaesu.stop_cw_cmd`.

[3] `radio-cat-rs` supports Flex native options such as `flex.retry_max`, `flex.retry_backoff_ms`, and `flex.verify_timeout_ms`.

[4] Log73 currently uses default backend behavior for these protocol options and does not provide UI fields for them.

---

## 18) Core feature checklist

- Browser UI served by backend executable
- Shared Basic Auth gate (optional)
- Multi-log create/edit/delete
- Multi-radio create/edit/delete
- Contest-driven exchange fields and validation
- CAT polling and live radio state updates
- Run/S&P CW function key messaging
- ESM Enter workflows
- ADIF export
- Cabrillo export
- Local pending/outbox cache for offline continuity
- Theme and zoom preferences

---

## 19) Known limitations / not yet implemented

- **Mark**, **Store**, and **Spot It** buttons are present but not implemented.
- Reliable/full dupe checking is not available while disconnected from backend.
- True robustness of same-port CAT+serial-CW setup may depend on platform/driver behavior.

---

## 20) Practical operating tips

- Run backend with the expected `--config-dir` and `--data-dir`, or use the platform defaults.
- Check the active data directory so your `log73.db` is where you expect.
- Before contest start:
  - verify radio CAT connection
  - verify CW keying path
  - open logger and send test messages
- If offline happens during operation, keep logging; pending QSOs will queue locally and commit later.

---

End of manual.
