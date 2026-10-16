# HTTP API contract

JSON endpoints live under `/api`. Success bodies contain the operation's result
without an `ok` envelope. The browser's `apiJson` returns that JSON unchanged;
204 and empty success responses return `null`. Downloads retain their text body,
content type, and `Content-Disposition` filename. Cached reference data can return
304 with no body; callers using conditional requests must handle that status.

Handler and request-extraction errors return JSON `{ "error": "message" }` with
400 for invalid requests, 404 for missing resources, and 500 for internal failures.
Existing radio/lease conflicts retain 409. Body size limits retain 413; HTTP
method errors retain 405. Error bodies can also include `code` and diagnostic
fields. HTTP authentication and WebSocket handshakes retain their own challenge
and upgrade contracts.

ADIF import errors retain `line` (the source record's line) and `errors`, an array
of `{line, error}` diagnostics. Contact validation errors retain `status: "failed"`.
Clients should keep these fields rather than reducing errors to message strings.
The browser exposes the parsed error body on `error.payload`; malformed JSON and
non-JSON errors still produce a useful fallback message.

This is a coordinated compatibility change for external direct API users:
stop testing `ok`, use HTTP status to distinguish failure, and read the direct
success values listed below. In-repository browser consumers retain the effective
values they previously received after legacy unwrapping. The Rust radio client
already parses registration directly and checks status only for lease operations;
its heartbeat/offline success response is now 204.

## Route and consumer inventory

The previous wire column records the contract before this transition. Where an
`ok` envelope was present, the last column records both the previous effective
`apiJson` value and the current wire value. Objects without `ok` were already
returned unchanged. All routes in the production API router are included;
`{kind}` expands to `cw`, `digital`, and `voice`.

| Method and route | Previous success wire | Effective/current success and consumers |
| --- | --- | --- |
| GET `/contest-rules` | array | array; CreateLogScreen |
| GET `/contest-settings` | rules object | rules object; CreateLogScreen, ExportLogScreen, ImportAdifScreen, useLoggerContext; default-contest fallback unchanged |
| GET `/bands`, `/modes` | arrays | arrays; useLoggerContext |
| GET, PUT `/config` | config object | config object; ConfigScreen |
| POST `/client-errors` | 204 | 204; errorReporting |
| GET `/supercheckpartial` | `{callsigns}` | `{callsigns}`; api.js; ETag/304 retained |
| GET `/dxcc` | `{dxcc}` | `{dxcc}`; api.js; ETag/304 retained |
| GET `/bandmap/spots` | `{sequence,spots}` | same; api.js, logger band map |
| POST `/bandmap/spots` | `{spot}` | `{spot}`; api.js, logger band map |
| DELETE `/bandmap/spots/{id}` | `{deleted}` | same idempotent result; api.js |
| GET, POST, DELETE `/dxcluster/spots` | `{spots}`, `{spot}`, `{deleted_ids}` respectively | same; api.js, logger cluster/band map |
| GET `/logs` | array | array; OpenLogScreen |
| POST `/logs`; GET, PUT `/logs/{id}` | log object | log object; CreateLogScreen, useLoggerContext, export/import screens |
| DELETE `/logs/{id}` | `{deleted}` | `{deleted:true}`; OpenLogScreen; absent log now 404 |
| GET `/logs/{id}/qso-count` | `{qso_count}` | same; OpenLogScreen |
| GET `/logs/{id}/stats` | `{ok:true,stats}` | stats object; useQsoStats |
| POST `/logs/{id}/adif`, `/logs/{id}/cabrillo` | text download | text download; apiDownload, ExportLogScreen, OpenLogScreen |
| POST `/logs/{id}/adif/import` | `{ok:true,imported,contacts}` | `{imported,contacts}`; ImportAdifScreen |
| GET `/logs/{id}/serial-allocation` | `{ok:true,state}` | serial state object; useSerialAllocator |
| POST `/logs/{id}/serial-allocation` | `{ok:true,allocation}` | allocation object; useSerialAllocator |
| GET `/logs/{id}/contacts` | array (including an empty array on load failure) | array; useContactsOutbox; failures now HTTP errors |
| POST `/logs/{id}/contacts` | `{ok:true,contact,contacts}` | `{contact,contacts}` for both single and batch input; useContactsOutbox |
| DELETE `/contacts/{id}` | `{ok:true,deleted}` | `true`; LoggerScreen; absent contact now 404 |
| GET `/audio-devices/input`, `/audio-devices/output` | arrays | arrays; CreateRadioScreen |
| GET `/radio-kinds`, `/serial-ports` | arrays | arrays; CreateRadioScreen, OpenLogScreen |
| GET `/radios` | array | array; CreateRadioScreen, OpenLogScreen |
| POST `/radios`; PUT `/radios/{id}` | `{ok:true,radio}` | radio object; CreateRadioScreen |
| GET `/radios/{id}` | radio object | radio object; CreateRadioScreen, useLoggerContext |
| DELETE `/radios/{id}` | `{ok:true,deleted}` | `true`; OpenLogScreen |
| GET `/radios/{id}/cw-labels` | labels object | same; direct API consumers |
| GET `/radios/{id}/message-labels` | `{cw,digital,voice}` | same; useLoggerContext |
| GET `/radios/{kind}-messages/default` | CW/digital: string; voice: `{ok:true,voice_messages}` | string for all three; CreateRadioScreen |
| POST `/radios/{kind}-messages/validate` | `{ok:true,labels}` | labels object for all three; CreateRadioScreen |
| PUT `/radio-clients/{client_instance_id}` | registration object | same `radio_id`, `lease_id`, heartbeat/timeout fields; Rust backend_client |
| POST `/radio-clients/{client_instance_id}/heartbeat`, `/offline` | `{ok:true}` | 204 (previous browser effective value `{}`); Rust backend_client reads status only |

Legacy imports, stats, message validation, radio creation, serial operations, and
contact mutations returned application errors as HTTP 200 with `ok:false`.
Downloads and radio updates already used error statuses but some retained legacy
bodies. These now all use the common error contract. Existing endpoints using
`ApiError` continue to use this contract; password-hashing infrastructure failures
now also correctly return 500.

## Contact commits and diagnostics

A committed contact always has both a `contact` field (the first committed
contact) and a `contacts` array. Diagnostic contest warnings are successful
commits and remain persisted, included in responses and WebSocket events, and
visible after reload. They do not trigger an outbox retry. Ordinary validation
errors retain their failure details and existing retry behavior; database or
transport failures remain errors. Integrity checks and immutable sent serials
still apply to diagnostic commits. Import validation stays strict.

No database migration is required: warning metadata uses the restored fresh
schema from prerequisite #35. An existing database created without that schema
must be recreated under the project's fresh-database policy.
