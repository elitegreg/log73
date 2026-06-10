# Prompt Template: Regenerate `docs/manual.md` (Log73 Operator Manual)

Use this prompt to regenerate a full, operator-focused Log73 manual similar in scope and detail to the existing `docs/manual.md`.

---

## Copy/paste prompt

You are writing a **standalone operator manual** for Log73.

### Audience and tone

- Primary audience: **contest operator** (not developers).
- Style: **tutorial-like**, practical, step-by-step.
- Assume production usage and shared-station workflows.
- Keep explanations plain-language and operator-friendly.

### Output target

- Write/overwrite: `docs/manual.md`
- Markdown only.
- Standalone document (do not require reader to open README or source code). Exceptions: Can reference `docs/keyboard-shortcuts.md`

### Hard requirements

1. Production-only operation.
2. Assume distributable backend layout and default Linux paths:
   - `/opt/log73/bin/log73-launcher`
   - config: `~/.config/log73/`
   - data/db/SCP/DXCC: `~/.local/share/log73/`
   - contest rules: `~/.local/share/log73/contest-rules`
3. Explain Basic Auth in plain language and include trust warning:
   - Any trusted operator with access can change credentials.
4. Include link to keyboard-shortcuts
5. Include **offline mode** for both cases:
   - backend unreachable at start/navigation
   - logger already open then websocket/backend drops
6. Include exactly what works vs what stops in offline mode.
7. Include pending/offline QSO behavior and durability in browser storage.
8. Include validation details for creating/editing logs and radios.
9. Explicitly document CAT + CW Serial same-port behavior.
10. Supported contests: **names only**.
11. Supported radios: **full list**, not summary. Look into radio-cat-rs crate. Add documentation for radio options.
12. Supported CW keyers: full list.
13. Mention known non-functional UI controls (`Mark`, `Store`, `Spot It`).

### Required sections (in this order)

1. Introduction: what Log73 is
2. Production deployment layout and startup assumptions
3. Backend startup + CLI options
4. Connecting with a browser
5. Configure Log73 screen
6. Quick tutorial flow (first-time operation)
7. Creating/editing logs (with validation details)
8. Creating/editing radios (with validation details)
9. Opening and using the logger
10. Offline mode (what works / what doesn’t)
11. Hotkeys and special input behaviors (complete)
12. ESM behavior summary/matrix
13. Log window selection/editing controls
14. Supported contests (names)
15. Supported CW keyers
16. Supported radios (full list)
17. Core feature checklist
18. Known limitations
19. Practical operating tips

### Source-of-truth inspection requirements

Inspect current project files before writing. At minimum:

- `README.md`
- `backend/src/main.rs` (routes, CLI, websocket, behavior)
- `backend/src/validation.rs` (validation rules)
- `backend/src/db.rs` (field constraints and defaults)
- `backend/src/radio_manager.rs` (CW keyer behavior, reconnect behavior, shared serial constraints)
- `backend/src/radio.rs` (mode mapping and websocket message types)
- `data/contest-rules/*.yaml` (contest names/params)
- `src/screens/ConfigScreen.jsx`
- `src/screens/OpenLogScreen.jsx`
- `src/screens/CreateLogScreen.jsx`
- `src/screens/CreateRadioScreen.jsx`
- `src/screens/LoggerScreen.jsx`
- `src/logger/MainWindow.jsx`
- `src/logger/LogWindow.jsx`
- `src/logger/components/*.jsx`
- `src/logger/mainWindowHelpers.js`
- `src/screens/loggerScreenHelpers.js`

### Runtime verification requirements

Re-check live/runtime-exposed values instead of guessing:

1. Backend CLI options:
   - run `log73-backend --help` (or `cargo run -- --help` in backend dev context)
2. Supported contests:
   - query `/api/contest-rules`
3. Supported radios:
   - query `/api/radio-kinds`
   - include full list in manual

### Accuracy rules

- Do **not** invent features.
- If behavior is partial/limited, say so explicitly.
- Keep wording operator-centric.
- Prefer practical examples over architecture deep dives.
- Ensure all hotkeys and special field-entry behaviors are included:
  - `Ctrl+O`
  - `Ctrl+K`
  - `Esc`
  - `PageUp/PageDown`
  - `F1..F12`
  - callsign Enter behavior for typed frequency/mode
  - ESM Enter and Alt+Enter behavior
  - tab behavior with fixed fields
  - log-window selection/edit shortcuts

### Radios with options (footnotes)

Include footnotes that note protocol-family options exist in `radio-cat-rs` (e.g. CI-V/Yaesu/Flex native option families), while clarifying whether Log73 UI currently exposes those options.

### Final self-check before saving

Confirm the draft includes:

- Production run commands for `/opt/log73` layout
- Basic Auth trust warning
- Offline mode both scenarios
- Pending/updating/failed local contact behavior
- Full contest list (names only)
- Full CW keyer list
- Full radio list
- Known non-functional buttons (`Mark`, `Store`, `Spot It`)

Then write `docs/manual.md`.

---

## Maintainer notes

- Keep this prompt file checked in so manual regeneration remains consistent.
- Update this prompt when major logger workflow, hotkeys, or radio/CW support changes.
