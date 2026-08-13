# Log73 Radio Client implementation plan

Status: design ready for implementation

This document expands the original `radioclient.md` proposal, records the decisions made during design, ties the proposal to the current code, and divides the implementation into reviewable tasks. It assumes a fresh database. No database migration is planned.

## Executive summary

Log73 will gain a native **Log73 Radio Client** application for operators who need the radio, CW keyer, sound device, and WSJT-X on the same computer as their browser instead of on the Log73 backend host.

The existing server-attached-radio model remains supported. Both models will use the same `radio-io` radio WebSocket and WSJT-X event pipeline:

- A **server-side radio** is owned and run by `log73-backend`, as it is today.
- A **client-side radio** is owned and run by `log73-radio-client` on the operator's computer.
- The backend remains the source of truth for logs and contacts, but the Radio Client is the source of truth for its radio configuration.
- A client-side radio registers a stable identity, name, loopback WebSocket URL, and read-only configuration snapshot with the backend. The backend never starts, stops, or edits that radio.
- The browser connects directly to the radio WebSocket URL returned for the selected radio. For a client-side radio, `127.0.0.1` therefore means the browser operator's computer, not the backend host.
- WSJT-X Logged ADIF arrives over UDP in `radio-io`, is sent over the radio WebSocket to the selected logger window, is durably queued by that window, and is then posted to the backend. This same route replaces the current direct backend ingestion route for server-side radios.
- CW and voice keying remain implemented by `radio-io`. Voice files for a client-side radio live on the client computer and are loaded into memory when the Radio Client starts.

The Open Log screen will clearly mark radios as **SERVER-SIDE** or **CLIENT-SIDE**, show client online/offline status, prevent an offline client-side radio from being opened, and explain that client-side configuration must be changed in the Radio Client application.

## Why this is needed

Log73's client/server architecture is deliberately multi-operator-first. A lightweight browser can run on Chromebooks, iPads, and other computers that cannot or should not install a heavyweight Windows-only contest logger. Today, multiple radios can be connected to the backend host through TCP or serial ports and shared by browser clients. That model works well for colocated SSB and CW stations and must continue to work.

It is less suitable in three important cases:

1. WSJT-X and similar digital-mode software need local access to a radio audio interface. If a combined USB serial/audio device is plugged into the backend host, the audio is there too. Multiple Linux desktop sessions can work around this, but that is cumbersome and does not translate well to ordinary Windows or macOS use.
2. Connecting every radio to one backend does not always scale and can require disruptive cabling or network serial arrangements.
3. Distributed multi-operator contest categories place operators and radios at different physical sites, where the central backend cannot directly reach every radio.

The Radio Client solves those cases without turning Log73 into a peer-to-peer logger and without removing the existing backend-owned radio option.

## Decisions and clarified requirements

The following decisions are settled for the first implementation:

1. **The Radio Client owns its configuration.** Radio settings, CW messages, voice messages, sound-device selections, WSJT-X listener settings, and local voice files are edited locally. The backend stores the identity and WebSocket URL plus a read-only snapshot needed by the existing logger UI. The web radio editor cannot update a client-side radio.
2. **WSJT-X is proxied through the open logger window.** `radio-io` receives the WSJT-X UDP Logged ADIF datagram and emits it on the radio WebSocket. The browser persists it before retrying an authenticated REST write to the backend.
3. **The existing server-side WSJT-X path will be changed to match.** Server-side and client-side radios will use one pipeline rather than maintaining direct-ingestion and browser-proxy variants.
4. **Client identities persist while offline.** A stable client UUID upserts the same backend radio row after restarts. Heartbeats determine runtime online/offline state. Stopping or disconnecting does not delete the row.
5. **The settings filename is `log73-radio-client.json`.** The `log72-radio-client.json` spelling in the original reference is treated as a typo.
6. **Backend Basic Auth credentials include both username and password.** The backend's current middleware requires both. Credentials apply only to outbound calls from the Radio Client to the backend.
7. **Local inbound traffic has no authentication.** The Radio Client binds only to `127.0.0.1`. Per the product decision, the first release will not add Origin checks, a capability token, SSL/TLS, or a secure-loopback proxy.
8. **HTTPS-hosted/mixed-content operation is out of scope.** The first release uses `ws://127.0.0.1:<ephemeral-port>` and ordinary HTTP backend URLs.
9. **The UI must call client-side radios client-side.** The term “remote” may still be useful internally, but user-visible labels will say **CLIENT-SIDE**, not merely “remote.”
10. **The WSJT-X transport is UDP, not XML-RPC.** This plan follows the protocol already implemented through `ham-radio-digital-interfacing`: the relevant incoming message is a WSJT-X UDP `LoggedAdif` message.

## Scope

### Included

- A cross-platform `log73-radio-client` Rust/Iced desktop executable.
- JSON settings in the normal Log73 config directory.
- The same path defaults and command-line path overrides used elsewhere in Log73.
- Local serial/TCP CAT control through `radio-io`.
- CAT, Winkeyer, or serial-line CW keying through `radio-io`.
- Voice-message playback to a selected local output device.
- WSJT-X UDP listening on the client machine.
- A loopback Axum server on an operating-system-assigned port.
- Radio registration, heartbeat, offline state, and read-only snapshots in the backend.
- Direct browser-to-loopback radio WebSocket connections.
- Frontend conversion of WSJT-X Logged ADIF into the existing persisted QSO outbox.
- Equivalent WSJT-X behavior for backend-owned radios.
- Client-side designation and status on the log/radio selection screen.
- Packaging, CI, tests, and user documentation for Linux, Windows, and macOS targets already listed in `dist-workspace.toml`.

### Not included in the first release

- TLS for the loopback WebSocket.
- Origin checks or authentication on the loopback server.
- Running the Radio Client on a different machine from the browser that controls it.
- Headless WSJT-X logging without an open logger window.
- Backend editing of client-owned radio settings.
- Backend-to-client command tunnels; the browser connects to the client directly.
- Downloading voice files from the backend. Client-side voice configuration and audio files are local by design.
- Multiple radios managed by one Radio Client process. The data model should not preclude this later, but the initial GUI and settings contain one radio.
- A database migration from an existing schema. Per repository guidance, the implementation will update the fresh schema only.

### Original proposal traceability

| Original detail                                                        | Planned treatment                                                                                                                                                                |
| ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Keep today's backend-attached multi-radio behavior                     | Retained as the SERVER-SIDE radio path and refactored to share, not replace, `radio-io`.                                                                                         |
| Serve a WebSocket to the frontend                                      | Axum binds `127.0.0.1:0` and serves the shared protocol at `/radiows`; the exact URL is registered with the backend.                                                             |
| CW message keying                                                      | Reuse CAT, Winkeyer, and serial-line keyers plus current `send_message`, `send_cw_text`, stop, WPM, and completion messages.                                                     |
| Voice message keying to a chosen output                                | Reuse `VoiceKeyer`/`VoicePlaybackThread`; enumerate the local output devices and preload local WAV bytes.                                                                        |
| Proxy WSJT-X log messages to Log73 upserts                             | Send Logged ADIF on the radio WebSocket, convert it to a normal pending QSO in the selected logger, and use the existing contact outbox/API.                                     |
| Share the `radio-io` crate                                             | Both `log73-backend` and `log73-radio-client` host the same manager, WebSocket protocol, keyers, voice cache, and WSJT-X manager.                                                |
| Use the launcher's GUI crates                                          | Use Iced `0.13` with the launcher's feature set, icon, async pattern, and native-platform approach.                                                                              |
| Use the same configurable directories/defaults                         | Reuse `log73-paths` and matching CLI overrides for config/data/app/log locations.                                                                                                |
| Save `log72-radio-client.json` beside launcher config                  | Save corrected `log73-radio-client.json` in `log73_paths::config_dir()`.                                                                                                         |
| Configure/Start/Stop main screen and scrolling messages                | Implemented as an explicit GUI state machine with guarded buttons and a bounded auto-scrolling event pane.                                                                       |
| Configure the same options as the current radio screen plus Basic Auth | Mirror all effective `CreateRadioScreen` fields and add backend URL, username, and masked password.                                                                              |
| Fetch CW/voice configuration and voice assets                          | Superseded by client ownership: configuration and files are local; the backend receives a read-only snapshot. Assets are still loaded into raw in-memory byte arrays before use. |
| Register name and WebSocket URL through a new REST API                 | Stable UUID upsert plus lease/heartbeat; backend stores identity, name, URL, and read-only snapshot.                                                                             |
| Persist the radio but do not let the backend update/start it           | Client rows persist offline, are not web-editable, and are excluded from the backend radio manager.                                                                              |
| Tell the frontend the WebSocket URL when opening a log                 | `/api/radios` returns the authoritative local or client URL; the frontend no longer overwrites its radio identity.                                                               |
| Choose direct REST versus browser proxy for WSJT-X failures            | Browser proxy selected; the existing durable contact outbox handles backend outages after ADIF conversion.                                                                       |

## Existing code and implications

The proposal is an extension and rearrangement of existing code, not a new radio stack.

| Area                       | Existing code                                                                                                       | Design implication                                                                                                                                                                                                                                                                                             |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Radio configuration UI     | `src/screens/CreateRadioScreen.jsx`                                                                                 | This is the field and validation baseline for the Iced Configure screen. It already covers driver, transport, digital mappings, WSJT-X, tuning increments, RIT, sound devices, CW keyer, and message text.                                                                                                     |
| Radio WebSocket            | `radio-io/src/websocket.rs`                                                                                         | Reuse the handler and protocol. It already validates frequency/mode/RIT/WPM/message commands and manages lazy `RadioManager` acquisition. Extend it with logger registration and WSJT-X events.                                                                                                                |
| Radio runtime              | `radio-io/src/radio_manager.rs` and its submodules                                                                  | CAT connection/reconnect, CW tasks, voice worker threads, RIT, and command dispatch remain shared by backend and Radio Client.                                                                                                                                                                                 |
| Protocol messages          | `radio-io/src/radio.rs`                                                                                             | Add WSJT-X target control, target-state, event, acknowledgment, and error variants here so both radio hosts serialize the same protocol.                                                                                                                                                                       |
| WSJT-X listener            | `radio-io/src/wsjtx.rs`                                                                                             | It already opens UDP unicast/multicast listeners, gates them on DATA mode and an active target, and emits Logged ADIF/errors. It should become owned by the radio WebSocket host rather than by the backend logger WebSocket.                                                                                  |
| Current WSJT-X persistence | `backend/src/wsjtx.rs`                                                                                              | Its direct subscription to `radio-io` goes away. The frontend converts radio-WebSocket Logged ADIF to an ordinary QSO, so normal backend contact validation, persistence, scoring, and events remain authoritative.                                                                                            |
| Browser radio connection   | `src/screens/loggerScreen/useRadioSocket.js`                                                                        | It already handles reconnect, idle ping/pong, status/state, and radio commands. It must treat `radio_ws_url` as authoritative, add logger identity, receive WSJT-X messages, and expose target state/control.                                                                                                  |
| Browser contact outbox     | `src/screens/loggerScreen/useContactsOutbox.js` and `loggerScreenHelpers.js`                                        | Feed converted WSJT-X contacts into this existing local-storage-first, retrying path. Do not create a parallel raw-event outbox.                                                                                                                                                                               |
| Backend logger WebSocket   | `backend/src/main.rs::handle_socket`                                                                                | It currently acquires a radio solely to drive backend WSJT-X. That ownership must be removed. It remains responsible for log events, scoring, band map, DX Cluster, and health checks.                                                                                                                         |
| Backend radio persistence  | `backend/src/db/radios.rs`, `models.rs`, and `schema.rs`                                                            | Add ownership/registration metadata while preserving bound, static SQL. Only backend-owned configs are loaded into the backend `RadioWebSocketState`.                                                                                                                                                          |
| Voice keyer                | `radio-io/src/voice_keyer.rs` and `voice_messages.rs`                                                               | It already loads fixed files into `Arc<[u8]>`, plays on worker threads, selects output devices, validates safe relative WAV paths, and prevents directory escapes. Template-based files currently fall back to disk at playback; that needs an in-memory relative-path cache for the Radio Client requirement. |
| Shared paths               | `paths/src/lib.rs`                                                                                                  | Reuse `config_dir()`, `data_dir()`, and `app_root()`. Add a Radio Client log-file helper if useful rather than duplicating OS rules.                                                                                                                                                                           |
| Desktop implementation     | `launcher/src/main.rs`                                                                                              | Match its Iced `0.13` setup, icon, theme, async `Task` pattern, settings persistence style, graceful shutdown, and process-status presentation where applicable.                                                                                                                                               |
| Selection screen           | `src/screens/OpenLogScreen.jsx`                                                                                     | Add explicit origin and lease state to option labels/details; disable remote editing and offline opening.                                                                                                                                                                                                      |
| Distribution               | workspace `Cargo.toml`, `dist-workspace.toml`, `Makefile`, WiX files, Linux packaging script, and release workflows | Add the new binary to normal builds and platform packages rather than requiring a source-only install.                                                                                                                                                                                                         |

Two current behaviors require deliberate changes:

- `useRadioSocket` currently constructs a WebSocket URL and always writes `radio_id=<backend database id>` into its query string. A client-owned server has its own runtime identity and may expose a URL without a radio ID. The backend-provided `radio_ws_url` must be authoritative; the frontend may add only logger-scoped parameters that are absent from it.
- The database currently enforces one enabled WSJT-X port globally. Client machines have independent loopback network namespaces and may all use port 2237. Port uniqueness must apply to backend-owned radios on the backend host, not across all client-owned radios.

## Target architecture

```text
                         authenticated REST
                  registration / heartbeat / QSOs
        +------------------------------------------------+
        |                                                v
+-------------------+      HTTP pages/API       +-------------------+
| operator browser  | <-----------------------> | log73-backend     |
| open logger       |                           | logs + scoring     |
+---------+---------+                           | radio registry     |
          |                                     +---------+---------+
          | ws://127.0.0.1                                  |
          | radio commands/state/WSJT-X                     | ws://backend
          v                                                 | server radio
+-------------------+                                       v
| Log73 Radio Client|                              +------------------+
| Axum + radio-io   |                              | backend radio-io |
+---------+---------+                              +--------+---------+
          |                                                  |
      CAT/CW/audio                                      CAT/CW/audio
          |                                                  |
       radio + local WSJT-X UDP                         server radio
```

The radio host is either `log73-radio-client` or `log73-backend`. From `radio-io` and the logger window's perspective, both expose the same protocol.

## Configuration ownership and representation

### Radio Client settings

The Radio Client is authoritative for:

- Backend base URL.
- Basic Auth username and password, blank when authentication is disabled.
- Stable `client_instance_id`, generated once and never regenerated by “reset radio defaults.”
- Radio display name.
- Radio driver and driver options.
- TCP/serial/none transport and its settings.
- DATA and RTTY CAT mode mappings.
- CW and SSB tuning increments.
- Clear-RIT-on-log behavior.
- CW keyer kind and serial/Winkeyer settings.
- CW message configuration.
- Voice input/output device IDs and voice message configuration.
- WSJT-X enabled state, bind address, UDP port, and multicast group.

The file is JSON and uses an explicit schema version. A representative shape is:

```json
{
  "schema_version": 1,
  "client_instance_id": "b55168f4-5d76-4eed-a17f-67b42167ac42",
  "backend": {
    "base_url": "http://logger.example:7300",
    "username": "operator",
    "password": ""
  },
  "radio": {
    "name": "40m run radio",
    "radio_kind": "elecraft_k4",
    "transport_kind": "serial",
    "tcp_host": "127.0.0.1",
    "tcp_port": 0,
    "serial_port": "/dev/ttyUSB0",
    "serial_baud_rate": 115200,
    "options": "",
    "data_mode": "DATA-USB",
    "rtty_mode": "RTTY",
    "wsjtx_enabled": true,
    "wsjtx_bind_address": "127.0.0.1",
    "wsjtx_port": 2237,
    "wsjtx_multicast_group": "",
    "cw_tuning_increment_hz": 20,
    "ssb_tuning_increment_hz": 100,
    "rit_clear_on_log": true,
    "voice_input_device_id": null,
    "voice_output_device_id": null,
    "cw_keyer_type": "cat",
    "winkeyer_serial_port": "",
    "cw_serial_port": "",
    "cw_serial_baud_rate": 9600,
    "cw_serial_line": "dtr",
    "cw_messages": "...",
    "voice_messages": "..."
  }
}
```

Implementation details:

- Default path: `log73_paths::config_dir().join("log73-radio-client.json")`, which is normally `~/.config/log73/log73-radio-client.json` on Linux.
- Voice files: `log73_paths::data_dir().join("voicekeyer")`, mirroring backend layout.
- Default log: `log73_paths::data_dir().join("log73-radio-client.log")`.
- CLI overrides: `--config-dir`, `--data-dir`, `--app-dir`, `--log-level`, and `--log-file`, with the same defaults as the backend/launcher path helpers. `--app-dir` is retained for consistent installed-layout discovery even if the first version has no installed data assets of its own.
- Save atomically by writing a sibling temporary file, flushing it, and renaming it.
- On Unix, create the settings file with user-only permissions where possible because it may contain a plaintext password. Never print the password or an Authorization header.
- Unknown future JSON fields should be ignored; a newer unsupported `schema_version` should produce an actionable error rather than silently discarding configuration.
- “Set defaults” resets radio fields but retains `client_instance_id`; credentials should require an explicit user action to clear.

### Backend read-only snapshot

The backend needs more than a name and URL in its API response because the current logger consumes tuning increments, `rit_clear_on_log`, WSJT-X enabled state, CW/voice message labels, and other radio capabilities. The recommended persistence is therefore:

- Continue storing the snapshot in the existing typed radio columns.
- Add `CONTROL_LOCATION` with values `backend` and `client`.
- Add nullable `CLIENT_INSTANCE_ID`, unique for client-owned rows.
- Add nullable `RADIO_WS_URL`.
- Add a registration timestamp for diagnostics if desired; online state remains lease-driven in memory.
- Treat every hardware/configuration column of a client-owned row as a read-only snapshot overwritten only by client registration.

This avoids a second parallel radio model and lets `/api/radios` continue returning one mostly flat object. It also means current logger logic can consume client radios with small, explicit origin/status additions.

Fresh-schema constraints should enforce:

- `CONTROL_LOCATION = 'backend'` implies no client instance or stored loopback URL.
- `CONTROL_LOCATION = 'client'` requires a nonempty client instance and WebSocket URL.
- Client instance IDs are unique.
- Enabled WSJT-X ports are unique only among backend-owned radios. Identical client-side ports on different computers are valid.

All database operations must keep the current static SQL and bound-parameter style in `backend/src/db/radios.rs`; no formatted user input belongs in SQL.

## Radio Client user experience

### Main screen

Use Iced and the same visual foundation and icon as the launcher. The main screen contains:

- **Configure** button.
- **Start** button.
- **Stop** button.
- A concise state summary: Not configured, Stopped, Starting, Running/registered, Running/backend unavailable, Stopping, or Error.
- The assigned local WebSocket address while running.
- The backend registration ID and last successful heartbeat when available.
- A bounded, timestamped, auto-scrolling message pane at the bottom. Keep, for example, the latest 1,000 entries so an overnight run cannot grow memory without limit.

Button rules:

- Start is disabled until a saved configuration passes local validation.
- Start is disabled in Starting, Running, and Stopping.
- Stop is enabled only in Starting or Running states.
- Configure is disabled while running so the configuration used by `RadioManager`, the WebSocket server, and the backend snapshot cannot diverge. Stop first to edit.
- Closing the window performs the same graceful stop sequence, with a bounded timeout.

“Running” means the loopback service and radio runtime are available. Backend registration has its own substatus. If the backend is temporarily unreachable, keep the local service alive, display a degraded state, and retry registration; do not require the operator to press Start again.

### Configure screen

Mirror the effective choices in `CreateRadioScreen.jsx`, implemented with native controls:

1. Backend URL, Basic Auth username, masked password, and **Test connection**.
2. Radio name and driver.
3. None/TCP/serial transport and conditional transport fields.
4. Driver-specific options.
5. DATA and RTTY mode mappings populated from driver capabilities.
6. WSJT-X enable, loopback/open bind choice, UDP port, and optional multicast group. Default to loopback/2237.
7. CW and SSB tuning increments and clear-RIT-on-log.
8. Sound subsystem, input device, and output device enumerated on this computer.
9. CW keyer choice, Winkeyer port, or serial-line keyer settings as applicable.
10. CW message editor with defaults and validation.
11. Voice message editor with defaults, validation, and a display of the local `voicekeyer` directory.
12. Save, Cancel, and Set defaults.

Validation should be shared with or moved into `radio-io` rather than reimplemented differently in Iced and `backend::validation`. In particular, normalize driver mode mappings, optional device IDs, serial settings, multicast settings, message syntax, and safe voice paths identically.

### Start sequence

1. Reload and validate the saved settings.
2. Ensure the configured data, log, and `voicekeyer` directories exist.
3. Enumerate and validate the configured serial and audio resources. Missing hardware is reported clearly; CAT reconnect behavior may still handle hardware that appears later.
4. Load voice WAV data into memory. Fixed and template-resolved paths must play from memory, not read from disk during transmission.
5. Build `VoiceKeyer`, `RadioManager`, `RadioWebSocketState`, and the WSJT-X manager for one local radio configuration.
6. Bind an Axum `TcpListener` to `127.0.0.1:0`, read the assigned port, and serve `/radiows`.
7. Construct `ws://127.0.0.1:<port>/radiows`. The single-radio handler must not require the backend database ID as a query parameter.
8. Register with the backend using the stable client instance ID, URL, name, and configuration snapshot.
9. Store the returned backend radio ID and lease ID only as runtime state; the stable identity remains the client UUID.
10. Start heartbeats and retry registration with capped exponential backoff if the backend becomes unavailable.

The physical CAT connection remains lazy, matching current `RadioWebSocketState`: it is acquired when a logger connects and released when the final logger disconnects. Start therefore does not monopolize the radio before it is used.

### Stop sequence

1. Stop accepting new WebSocket connections.
2. Close active radio sockets and stop WSJT-X targeting/listening.
3. Release/shut down `RadioManager`, CW tasks, and voice playback; stop any active transmission/playback.
4. Send a best-effort offline request to the backend.
5. Stop heartbeat and HTTP tasks and close the listener.
6. Retain the backend row and stable client UUID.

If the process crashes or loses the network, the backend lease expires and produces the same offline result.

## Registration, heartbeat, and backend lifecycle

### REST API

Suggested endpoints, all covered by the backend's existing Basic Auth middleware:

#### `PUT /api/radio-clients/{client_instance_id}`

Upserts the one client-owned radio for this installation.

```json
{
  "name": "40m run radio",
  "radio_ws_url": "ws://127.0.0.1:49152/radiows",
  "config": {
    "radio_kind": "elecraft_k4",
    "transport_kind": "serial",
    "serial_port": "/dev/ttyUSB0",
    "serial_baud_rate": 115200,
    "wsjtx_enabled": true,
    "wsjtx_bind_address": "127.0.0.1",
    "wsjtx_port": 2237,
    "cw_messages": "...",
    "voice_messages": "..."
  }
}
```

The actual config payload should reuse the complete shared radio payload rather than a partial ad hoc type. The server validates it, creates or updates the row with bound SQL, marks it online in the runtime registry, and returns:

```json
{
  "radio_id": 17,
  "lease_id": "d67853c0-0c52-4963-a626-b0bcb9880727",
  "heartbeat_interval_seconds": 10,
  "lease_timeout_seconds": 30
}
```

A new registration supersedes the previous lease for the same client UUID. This makes a second accidental process start deterministic: the latest successful registration owns the lease and stale heartbeats are rejected.

#### `POST /api/radio-clients/{client_instance_id}/heartbeat`

Body contains the `radio_id` and current `lease_id`. A valid heartbeat refreshes an in-memory monotonic deadline. It does not rewrite the database every ten seconds.

#### `POST /api/radio-clients/{client_instance_id}/offline`

Best-effort graceful stop using the same lease fields. It marks the runtime entry offline immediately but does not delete it.

Use meaningful HTTP status codes for malformed data, wrong credentials, stale leases, and conflicts. The Radio Client should distinguish authentication failure from temporary reachability failure in its event log.

### Backend runtime registry

Add a small cloneable registry to `AppState`, keyed by backend radio ID/client UUID, containing the current lease ID, last-seen monotonic time, and online flag. A background task expires entries after the lease timeout and emits a status change only on transitions.

After a backend restart all persisted client-side rows begin offline. Active Radio Clients will re-register through their retry loops. A stale persisted WebSocket URL is never considered usable until a new lease is active.

`GET /api/radios` and `GET /api/radios/{id}` add:

```json
{
  "control_location": "client",
  "client_online": true,
  "radio_ws_url": "ws://127.0.0.1:49152/radiows"
}
```

For backend-owned radios, `control_location` is `backend`, `client_online` is not needed (or is always true), and `radio_ws_url` remains the generated `/radiows?radio_id=<id>` URL.

### CRUD behavior

- The existing create endpoint creates only backend-owned radios.
- Update rejects client-owned rows with “Configure this radio in Log73 Radio Client.”
- Client registration is the only operation that updates the read-only snapshot.
- Explicit deletion of an offline client-side row is allowed with confirmation. Deleting an online client-side row is rejected. If its still-configured client later starts, registration creates the row again, possibly with a new backend numeric ID.
- Backend radio mutation and lazy runtime registration continue to use `RadioWebSocketState::begin_mutation`.
- Client-side rows are never inserted into the backend's `RadioWebSocketState`, so opening them cannot cause backend CAT/serial activity.

## Radio WebSocket protocol and ownership refactor

### Connection identity

The browser receives the exact `radio_ws_url` from `/api/radios/{id}`.

- Backend radio example: `/radiows?radio_id=4`
- Client-side radio example: `ws://127.0.0.1:49152/radiows`

`useRadioSocket` resolves that URL but does not replace its `radio_id`. It adds `logger_id` and `log_id` query parameters. The local single-radio handler ignores backend database identity; the backend handler still uses its embedded `radio_id`.

The handler validates nonempty logger identity and a positive log ID before registering the connection as a WSJT-X candidate. Radio CAT control can still connect independently in tests, but production logger connections provide both.

### Existing messages retained

Keep the existing `radio-io` protocol messages and validation:

- Browser to radio: `ping`, `set_frequency`, `set_mode`, `rit_clear`, `rit_increment`, `rit_decrement`, `send_message`, `send_cw_text`, `stop_keying`, and `set_wpm`.
- Radio to browser: `radio_status`, `radio_state`, `pong`, and `message_sent`.

### WSJT-X messages moved/added

Move target selection from the backend logger WebSocket into `RadioClientMessage`:

```json
{ "type": "set_wsjtx_target", "enabled": true }
```

Add radio-to-browser messages:

```json
{
  "type": "wsjtx_target",
  "logger_id": "browser-logger-uuid-or-null",
  "log_id": 42
}
```

```json
{
  "type": "wsjtx_logged_adif",
  "event_id": "radio-host-generated-uuid",
  "log_id": 42,
  "text": "<QSO_DATE:8>...<EOR>"
}
```

```json
{
  "type": "wsjtx_error",
  "log_id": 42,
  "message": "Invalid WSJT-X datagram ..."
}
```

After accepting the event into the normal contact outbox, acknowledge receipt:

```json
{ "type": "wsjtx_event_received", "event_id": "..." }
```

The acknowledgment is operational telemetry, not a deletion trigger for a Radio Client queue: the first release does not persist WSJT-X messages in the Radio Client. The contact outbox remains responsible for browser-local persistence.

### Target arbitration

Preserve current behavior from `radio-io/src/wsjtx.rs`:

- Logger connections register as candidates but start untargeted.
- A logger becomes the WSJT-X target only when its DATA-mode WSJT-X checkbox is enabled.
- Later logger windows do not steal the selected target automatically.
- A logger can explicitly claim or clear the target.
- Only one target exists per radio.
- Closing the selected logger clears the target; it does not silently reassign another logger.
- The UDP listener runs only when WSJT-X is enabled, the radio reports DATA mode, and a target exists.
- Manual entry remains locked in the targeted DATA-mode logger as it is today.

Move the `WsjtXManager` ownership into the radio WebSocket host/state so both executables get this behavior. `backend::handle_socket` should no longer acquire a `RadioHandle`, register a WSJT-X target, or release a radio. Its existing backend `set_wsjtx_target` message can be removed after the frontend switches to the radio socket.

`WsjtXEvent::LoggedAdif` should identify the target logger/log and carry a newly generated stable event UUID. Route it only to the target radio socket; do not broadcast a QSO to every browser connected to the radio.

## WSJT-X ingestion through the normal QSO path

On `wsjtx_logged_adif`, the selected logger verifies the log ID and a 64 KiB input limit, parses exactly one complete QSO record, and creates an ordinary pending QSO. It uses the radio-host event UUID as `meta.clientId`, sets the current session/log metadata, `source: "wsjtx"`, and `force: true`, then inserts the QSO into the existing contact state. The source marker prevents generic local-contact normalization from changing preserved ADIF strings after a page reload. The existing contact outbox provides local persistence, retry, and `POST /api/logs/{log_id}/contacts`; no WSJT-X-specific endpoint or raw-event outbox exists.

Every QSO-record ADIF field is copied into `qso.adif` with an uppercase field name and its parsed string value. The sole conversion is `QSO_DATE` plus `TIME_ON` into numeric `QSO_DATE_TIME_ON`; those two source fields are removed. `QSO_DATE_OFF`, `TIME_OFF`, `FREQ`, `CONTEST_ID`, `OPERATOR`, `SUBMODE`, comments, propagation fields, application fields, underscore-prefixed fields, and unknown future fields remain unchanged. The current operator and Cabrillo transmitter ID are added only when their corresponding incoming fields are absent.

The logger acknowledges `wsjtx_event_received` after accepting the event locally and ignores a repeated event already present under the same local client ID. The accepted product tradeoff is that the ordinary contact POST is not made transactionally idempotent for the rare case where the backend commits but its HTTP response is lost.

An open logger window remains required. With no target, `radio-io` does not run the listener. This matches current targeting semantics and the chosen browser-proxy architecture.

### Error path

- UDP/socket/protocol errors emitted by `radio-io` travel as `wsjtx_error` on the target radio socket and use the existing logger notification/error-reporting patterns.
- Normal contact validation/database errors are returned to the existing outbox and handled by its existing failure/retry behavior.
- Do not include Basic Auth credentials or full ADIF payloads in ordinary info logs. Debug logging should retain the repository's existing truncation rules.

## CW and voice keying

### CW

No separate proxy protocol is required beyond the existing radio WebSocket:

- Function-key messages use `send_message` with mode, keys, and field values.
- Arbitrary text uses `send_cw_text`.
- `stop_keying` and `set_wpm` retain current behavior.
- CAT, Winkeyer, and serial DTR/RTS implementations continue to live in `radio-io`.
- Message completion returns `message_sent` so ESM/UI behavior stays unchanged.

The client-owned CW message text is included in the backend read-only snapshot so the web UI can display matching labels. The Radio Client executes against its own in-memory authoritative config.

### Voice

Client-side voice files live under the Radio Client's data directory `voicekeyer/`. This deliberately revises the original “fetch voice files from the backend” idea: because the Radio Client owns configuration and playback hardware, it also owns the audio assets.

At Start:

- Validate configured voice messages and safe relative `.wav` paths.
- Recursively index files under `voicekeyer/` without following paths outside the canonical root.
- Load bytes into an `Arc<[u8]>` cache keyed by normalized relative path.
- Resolve fixed message entries to their cached bytes.
- Resolve template paths such as `{OPERATOR}/CQ.wav` against the same cache during playback, never by reading the disk in the transmit path.
- Reject duplicate normalized paths and enforce per-file/total memory limits with clear errors.

`VoicePlaybackThread` remains the blocking audio boundary. Output device IDs come from the client machine. The existing input device field remains in the mirrored configuration even though first-release playback only uses output; it preserves current radio configuration compatibility and room for recording tools.

Changing voice files or message configuration requires Stop, Configure/replace files, and Start, making the in-memory set deterministic.

Backend-owned voice radios retain their current backend data-directory behavior, but should use the same cache abstraction after the refactor.

## Open Log/radio selection changes

`OpenLogScreen.jsx` must make ownership impossible to miss.

Example option labels:

- `[SERVER-SIDE] Main K4 - Elecraft K4 - serial /dev/ttyUSB0 @ 115200`
- `[CLIENT-SIDE · ONLINE] Greg laptop K4 - Elecraft K4`
- `[CLIENT-SIDE · OFFLINE] Remote 20m station - Elecraft K3`

Also show a detail/help line for the selected radio:

- Server-side: “Radio hardware is controlled by the Log73 backend.”
- Client-side online: “Radio hardware is controlled by Log73 Radio Client on the operator computer.”
- Client-side offline: “Start Log73 Radio Client on that computer before opening this radio.”

Behavior:

- Poll `/api/radios` periodically while the selection screen is open, or consume a small status event, so lease changes appear without a page reload.
- Do not auto-select an offline client-side radio when an online radio is available.
- Disable **Open** for an offline client-side selection and show the reason.
- Disable **Edit** for client-side radios and label/tooltip it “Configure in Radio Client.”
- Keep **Create** for backend-owned radios. Add a note that client-side radios register automatically and should not be created here.
- Allow explicit deletion only when the client is offline and after confirmation that it will reappear if the client registers again.
- Keep the CLIENT-SIDE designation in logger context/status after opening, not only on the selection page, so an operator troubleshooting CAT knows which computer owns it.

A globally online client radio may still belong to another operator's computer. Clear names are therefore important. If a browser chooses the wrong operator's client-side radio, its loopback connection will fail locally and the logger must report “Radio Client is not running on this computer or this is another operator's client-side radio,” not a generic WebSocket failure.

## Failure handling and concurrency

### Backend unavailable

- The Radio Client keeps its loopback server alive and retries registration with exponential backoff capped around 30 seconds.
- A 401/403 is shown as a credentials/configuration error and retried slowly rather than flooding the backend.
- The browser's WSJT-X outbox persists received events and retries when REST service returns.
- Existing manual contact outbox behavior is unchanged.

### Radio Client unavailable

- Heartbeat expiry marks the row offline.
- The Open screen prevents new sessions.
- An already-open logger's radio socket reconnect loop continues, visibly disconnected, and recovers if the same client restarts and the logger refreshes its changed ephemeral URL. Because restart changes the port, registration status changes should prompt/refetch the radio record; a simple initial implementation may require returning to Open Log, but automatic URL refresh is the preferred acceptance behavior.

### Backend restart

- Persisted client rows start offline.
- Radio Clients re-register and receive their existing numeric radio IDs because upsert uses stable UUIDs.
- Open selection screens refresh status.
- Logger backend sockets and outboxes follow existing reconnect behavior.

### Duplicate Radio Client process

- The latest registration gets a new lease ID.
- Old-process heartbeats cannot keep the row online.
- The GUI reports lease replacement if it receives a conflict.
- A future enhancement could use an OS instance lock, but backend lease correctness is required regardless.

### Multiple logger windows

- `RadioManager` reference counting remains unchanged.
- One WSJT-X target is chosen using the existing explicit arbitration rules.
- Radio state/status broadcasts reach all radio sockets.
- Logged ADIF reaches only the selected target logger.

### Resource limits

Retain current WebSocket validation limits for request IDs, text, fields, nesting, frequency, RIT, and WPM. Add bounded event logs, WSJT-X ADIF size, WSJT-X outbox depth, WebSocket channel capacity, voice-file size, and total voice-cache memory. Lag/drop conditions must emit errors; dropping a Logged ADIF event silently is not acceptable.

## Security boundaries selected for this release

- The local Axum listener binds to the literal IPv4 loopback address with an ephemeral port. It must never honor a config value that changes the HTTP/WebSocket listener to `0.0.0.0`.
- WSJT-X has a separate configurable UDP bind address; preserving the existing loopback/open option is intentional for multicast/network WSJT-X setups.
- Local REST endpoints are unnecessary; expose only `/radiows` and an optional non-sensitive `/health` route.
- No local Basic Auth, token, Origin check, TLS, or certificate management is required by product decision.
- Backend calls use Basic Auth only when username/password are configured.
- Redact credentials and Authorization headers from logs and errors.
- Validate the registered URL as a `ws://127.0.0.1:<nonzero-port>/radiows` loopback URL before returning it to browsers. The backend does not connect to it.
- Continue safe path/canonicalization checks for voice files.
- Continue static SQL with bound parameters for registration, snapshot, and lease-related persistence.

## Packaging and repository integration

Create a new workspace package, preferably `radio-client/`:

- Package/binary name: `log73-radio-client`.
- Core dependencies: Iced with the same features as the launcher, Axum WebSocket support, Tokio, Reqwest with rustls/json, Serde/Serde JSON, tracing, `log73-paths`, and `radio-io`.
- Reuse `static/log73-icon-512.png`.
- Add Cargo-dist/WiX metadata with its own stable upgrade/component GUIDs.
- Add Make targets: `radio-client-build`, `radio-client-test`, `radio-client-fmt`, `radio-client-lint`, `radio-client-run`, and aggregate `radio-client`.
- Include its checks in `make ci` and add it to relevant reusable workflows.
- Add `log73-radio-client` to Linux DEB/RPM package contents and a separate desktop entry named “Log73 Radio Client.”
- Ensure release artifacts include the executable on all configured targets.
- Decide during packaging implementation whether the normal Log73 installer exposes separate launcher shortcuts or one feature tree; do not hide the Radio Client binary from ordinary users.

The native Linux packaging script currently uses generated-file shell writes and destructive recreation of a known target staging directory. Implementation should modify it carefully without widening deletion targets.

## Documentation

Update the README and generated manual sources to cover:

- When to use server-side versus client-side radio control.
- Installing and launching the Radio Client.
- Settings/config/data/log paths by OS.
- Backend URL and Basic Auth configuration.
- Voice-file directory layout and restart-to-reload behavior.
- WSJT-X UDP settings and the requirement for an open targeted logger in DATA mode.
- Meaning of Registered, backend unavailable, CAT offline, and client offline states.
- The fact that `127.0.0.1` refers to the browser/Radio Client computer.
- Troubleshooting client-side selection from the wrong operator computer.
- The explicit first-release lack of HTTPS/mixed-content support.

Regenerate `docs/help/*.html` through the existing `make help` flow when implementation documentation is complete.

## Testing strategy

### `radio-io` tests

- `RadioConfig` serialization/deserialization shared by settings and registration.
- Single-radio WebSocket handler without a query radio ID.
- Logger identity validation and registration/release.
- First-target, explicit-target, clear-target, and target-disconnect behavior through radio WebSockets.
- Logged ADIF is delivered only to the selected logger.
- Local/server host protocol serialization is identical.
- UDP listener starts only for enabled DATA mode with a target.
- WSJT-X errors serialize and route correctly.
- Fixed and template voice paths use the in-memory cache after source files are removed/changed.
- Voice cache path escape, duplicate, missing file, and memory-limit failures.

### Radio Client tests

- Default paths and CLI override resolution.
- JSON round trip, schema version handling, stable UUID retention, atomic saves, and corrupt-file recovery messaging.
- Validation parity for every conditional configuration field.
- Start/stop state transitions and button enablement.
- Ephemeral loopback bind and reported URL.
- Registration payload redaction, Basic Auth use/omission, and HTTP error classification.
- Registration retry/backoff, lease heartbeat, stale lease, graceful offline, and backend restart.
- Bounded GUI event log.
- Graceful shutdown with active CAT/CW/voice/WSJT-X work.

### Backend tests

- Fresh schema creates valid backend/client radio constraints.
- Client registration creates a row with bound data.
- Re-registration by UUID keeps the numeric radio ID and replaces the snapshot/URL.
- A second lease supersedes the first.
- Heartbeat and timeout transition online state without deleting the row.
- Client rows are excluded from backend `RadioWebSocketState` startup.
- Client rows cannot be web-edited or deleted while online.
- Backend radio CRUD behavior remains unchanged.
- Duplicate WSJT-X ports are rejected for two backend radios but allowed across client radios.
- WSJT-X event ingestion validates, enriches, scores, broadcasts, and returns a committed contact.
- Retrying the same event ID returns one QSO and one scoring effect.
- Malformed/oversized ADIF and event IDs fail safely.
- Existing backend WebSocket no longer depends on acquiring a radio.

### Frontend tests

- Radio summaries and details say SERVER-SIDE/CLIENT-SIDE and online/offline.
- Offline client selection cannot open.
- Client Edit is disabled with the local-configuration explanation.
- The exact backend-provided radio URL is preserved; logger/log parameters do not overwrite its query.
- Radio socket target messages update the existing DATA-mode lock UI.
- WSJT-X events are persisted before upload, resume after reload, retry transient failures, stop retrying permanent failures, and enforce limits.
- Duplicate event delivery produces one pending outbox entry.
- Radio socket reconnection and existing CAT/message behavior remain intact.

### End-to-end/manual matrix

At minimum exercise:

- Linux backend radio and Linux client-side radio.
- Windows Radio Client with local serial/audio devices and a remote Linux backend.
- macOS Radio Client startup/registration and available serial/audio enumeration.
- Basic Auth disabled and enabled.
- Backend unavailable at Start, lost while running, and restarted.
- Radio Client stopped/crashed/restarted while selection and logger pages are open.
- Two browser logger windows contending for one WSJT-X target.
- WSJT-X Logged ADIF while the backend is unreachable, followed by successful queued ingestion.
- Same UDP port used by client radios on two different computers.
- CW CAT/Winkeyer/serial modes and voice output cancellation.

Before any implementation commit, `make ci` must pass as required by `AGENTS.md`. Packaging smoke checks are additional because `make ci` does not currently build native installers.

## Acceptance criteria

The feature is complete when:

1. Existing backend-owned radios can still be created, edited, selected, CAT-controlled, CW-keyed, voice-keyed, and used with WSJT-X.
2. An operator can configure and start `log73-radio-client`; it binds only to loopback on an assigned port and registers without a manually chosen port.
3. Restarting the same client updates the same backend radio row and ephemeral URL.
4. The backend never opens hardware or starts a radio manager for a client-side row.
5. Open Log visibly distinguishes SERVER-SIDE and CLIENT-SIDE radios, shows lease status, and blocks offline client radios.
6. Client-side Edit is disabled in the web UI with direction to configure locally.
7. A logger connected to a client-side radio receives radio state and can issue all existing radio/CW/voice commands.
8. Voice files used by normal and template messages are in memory before transmission.
9. WSJT-X Logged ADIF for either radio origin travels radio-io → radio WebSocket → selected logger outbox → backend and produces a normal committed/scored QSO.
10. A backend outage after browser receipt leaves the WSJT-X QSO pending in the existing contact outbox for retry.
11. Client heartbeat expiry changes the radio to offline without deleting it, and re-registration restores it.
12. Basic Auth works for registration, heartbeat, and normal QSO submission without exposing credentials in logs.
13. Linux/Windows/macOS release outputs include the Radio Client, and all automated tests/`make ci` pass before commit.

## Alternatives considered and future improvements

### Direct Radio Client REST ingestion

This is simpler at first glance, but the client would need to know and track the active log, implement its own durable disk queue, and coordinate multi-logger target selection. It was rejected in favor of the chosen browser proxy, which aligns the QSO with the open logger and its established reconnect behavior.

### Backend-owned client configuration and audio downloads

This would centralize editing but makes local device enumeration awkward, requires an audio asset API, and creates two-way synchronization/failure rules. It was superseded by the decision that the Radio Client owns settings and files. The backend snapshot remains available for display and logger behavior only.

### WebSocket relay through the backend

Relaying every CAT/keyer message would work across networks but adds latency, backend state, and a second protocol hop. Direct browser loopback is the intended design.

### Headless Radio Client WSJT-X queue

A disk-backed client queue could allow logging with no browser window. That is a possible later operating mode, but it needs an explicit log-selection UI and conflict semantics. It is not silently included in this release.

### Multiple radios per client process

The registration API can later accept a stable per-radio key under one client UUID, and the GUI can become a list. The first version intentionally operates one radio to keep lifecycle, ports, and device ownership obvious.

### Secure loopback and HTTPS

Origin validation, capability URLs, local TLS, and public-site-to-loopback restrictions were considered and explicitly deferred. If Log73 later requires HTTPS-hosted operation, this decision must be revisited rather than assuming the plain `ws://` design works unchanged.

### General contact idempotency

The existing contact outbox can retry after a lost success response, which could duplicate either a manual or WSJT-X QSO. Product direction accepts this small surface area for now; do not add schema or endpoint complexity unless that decision changes.

### Automatic open-logger URL refresh

Because the Radio Client port changes on restart, an open logger should eventually receive/refetch the new URL automatically. If this makes the first slice too large, returning to Open Log may be an interim behavior, but automatic recovery remains the target and should be tracked explicitly.

## Resolved questions and implementation assumptions

- **Who owns radio settings?** The Radio Client; the backend copy is read-only.
- **Where do client-side voice files live?** In the Radio Client data directory, not on the backend.
- **How are WSJT-X contacts saved?** The selected logger converts Logged ADIF into a normal pending QSO and uses the existing contact API.
- **Does the existing server path stay different?** No; it is refactored to the same radio-WebSocket path.
- **What persists when a client stops?** Stable identity, row, name, URL/snapshot history; online state expires.
- **Can the backend edit the client radio?** No.
- **Is local inbound authentication required?** No; loopback binding is the selected boundary.
- **Must HTTPS work?** No, not in the first release.
- **How is the radio identified to operators?** Explicit CLIENT-SIDE/SERVER-SIDE labels plus online state.
- **Are migrations part of the work?** No. Update the fresh schema and tests only. If preserving existing databases becomes a requirement later, stop and design a migration separately.

## Task breakdown and division of work

The tasks below are deliberately small enough for separate commits/reviews. Dependencies are called out so work can be divided without creating conflicting protocol definitions.

### Workstream A — shared radio foundation

**A1. Shared configuration model and validation**

- Make the shared radio configuration deserializable.
- Move/extract radio payload validation and mode normalization needed by both backend and Radio Client into an appropriate shared module.
- Add defaults/builders so backend IDs and client runtime IDs are not confused.
- Add parity tests.
- Dependency: none.

**A2. In-memory voice asset cache**

- Add safe recursive relative-path indexing and memory limits.
- Route fixed and templated voice playback through cached bytes.
- Preserve backend-owned voice behavior.
- Add disk-change/removal-after-load tests.
- Dependency: A1 only if config helpers move simultaneously.

**A3. Radio WebSocket logger/WSJT-X protocol**

- Define all target/event/error/ack message variants.
- Move WSJT-X manager ownership into radio WebSocket state.
- Add single-radio handler support and targeted event routing.
- Preserve target arbitration and radio lifecycle reference counting.
- Dependency: A1; protocol shape should land before frontend/client work.

### Workstream B — backend remote registry and ingestion

**B1. Fresh database and radio record model**

- Add control location, client identity, stored URL, and snapshot rules.
- Scope WSJT-X port uniqueness correctly.
- Add static, bound registration/upsert queries and tests.
- Filter client radios out of backend runtime initialization.
- Dependency: A1.

**B2. Registration lease service and REST API**

- Implement registration, heartbeat, offline, lease replacement, expiration task, and API status projection.
- Integrate Basic Auth/redacted tracing.
- Add lifecycle tests with paused Tokio time where practical.
- Dependency: B1.

**B3. Read-only client-radio CRUD semantics**

- Reject edits, handle explicit offline deletion, and return actionable errors/status codes.
- Preserve local `begin_mutation` behavior.
- Dependency: B1–B2.

**B4. WSJT-X frontend QSO bridge**

- Treat the radio URL as authoritative and move target state/control to the radio WebSocket.
- Strictly convert one Logged ADIF record into a normal pending QSO while preserving all fields except the combined on-time pair.
- Feed it into the existing contact outbox, acknowledge local receipt, report errors, and suppress repeated local event IDs.
- Dependency: A3.

**B5. Remove backend logger-WebSocket radio ownership**

- Remove its radio acquire/release and WSJT-X target handling.
- Retain log/band-map/DX Cluster behavior and tests.
- Host server-side WSJT-X through the extended radio WebSocket state.
- Dependency: A3 and B4.

### Workstream C — Radio Client desktop application

**C1. Crate, CLI, paths, tracing, and JSON settings**

- Scaffold workspace package and Iced shell.
- Implement defaults, overrides, atomic persistence, stable UUID, credential redaction, and tests.
- Dependency: A1.

**C2. Configure screen**

- Implement backend, driver/transport, WSJT-X, tuning/RIT, audio, keyer, and message controls.
- Enumerate local devices and validate/save.
- Add Test connection and validation tests.
- Dependency: C1 and A1.

**C3. Main screen and lifecycle state machine**

- Implement Start/Stop/Configure enablement, status details, bounded event log, and graceful close.
- Start loopback Axum/radio runtime/voice cache and stop it deterministically.
- Dependency: A2–A3 and C1–C2.

**C4. Backend registration and lease client**

- Add authenticated registration, heartbeat, offline, backoff, lease replacement handling, and GUI events.
- Keep service alive while backend is unavailable.
- Dependency: B2 and C3.

### Workstream D — browser integration

**D1. Authoritative radio URL and radio-socket target state**

- Preserve the returned URL/query, add logger/log identity, move WSJT-X target control/state from backend socket to radio socket, and keep reconnect/health behavior.
- Dependency: A3.

**D2. WSJT-X browser regression coverage**

- Verify ADIF field preservation, normal contact-outbox submission, target reconnect behavior, receipt acknowledgment, and actionable errors.
- Dependency: B4.

**D3. Client-side selection designation and controls**

- Add labels/details/status refresh, offline Open blocking, read-only Edit behavior, delete confirmation, and logger-context designation.
- Add tests for summary/selection helpers.
- Dependency: B2–B3 API projection; can be developed against fixtures.

**D4. Regression cleanup**

- Remove obsolete backend-socket WSJT-X target handling and update related helpers/tests/manual-entry locking.
- Dependency: D1–D2 and B5.

### Workstream E — delivery and verification

**E1. Build/CI integration**

- Add Make targets and workflow coverage; ensure workspace format/clippy/test includes the client.
- Dependency: C1; final assertions depend on all code work.

**E2. Platform packaging**

- Add Cargo-dist/WiX metadata, Linux package binary/desktop entry, icons, and artifact smoke checks.
- Dependency: C3 and E1.

**E3. User documentation**

- Update README/manual/help sources with setup, operation, paths, voice, WSJT-X, statuses, and troubleshooting.
- Dependency: UI/API wording stable after C2, C4, and D3.

**E4. End-to-end verification**

- Run the manual matrix, fix cross-platform/device issues, run `make ci`, build packages, and record any explicitly deferred issue.
- Dependency: all prior tasks.

### Recommended sequencing

1. Land A1 and the B1 schema/model changes together or consecutively.
2. Land A3 protocol/ownership before parallel work on B5, C3, and D1.
3. B2 and C1/C2 can proceed in parallel once A1 is stable.
4. B4 and D2 form one vertical WSJT-X delivery slice and should be reviewed together.
5. Complete D3 after backend status projection, then remove obsolete paths in B5/D4.
6. Finish voice caching and client lifecycle before packaging.
7. Documentation, full CI, packaging smoke tests, and the manual matrix close the feature.
