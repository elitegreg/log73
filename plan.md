# Log73 path defaults plan

## Goal

Configure all path defaults for all supported OSes. Settings are for all paths used by the backend and launcher.

Linux defaults:

- Config: `~/.config/log73/`
- Data: `~/.local/share/log73/`
- Database/SCP/DXCC: under `~/.local/share/log73/`
- Contest rules: `~/.local/share/log73/contest-rules`
- Application root: `/opt/log73/`

On macOS and Windows, adapt those locations to appropriate OS-specific config/data/application directories.

## Development plan

1. Add a shared Rust path helper crate/module for Log73 defaults:
   - Platform config dir default.
   - Platform data dir default.
   - Contest rules dir as `<data-dir>/contest-rules`.
   - DB path as `<data-dir>/log73.db`.
   - App root detection from `${APP_ROOT}/bin/log73-backend`.
   - Linux fallback app root `/opt/log73`; OS-specific fallbacks for macOS/Windows.

2. Backend changes:
   - Replace `--contest-rules-dir` with `--config-dir` and `--data-dir`.
   - Default both dirs via shared path helper.
   - Create config/data dirs on startup.
   - Load contest rules from `<data-dir>/contest-rules`.
   - Load `MASTER.SCP` and `cty.dat` from `<data-dir>`.
   - Open SQLite DB at `<data-dir>/log73.db` instead of the current working directory.

3. Launcher changes:
   - Store launcher settings under the common Log73 config dir instead of `log73-launcher`.
   - Add/use config-dir and data-dir settings.
   - Pass `--config-dir` and `--data-dir` to backend.
   - Remove old contest-rules/data autodiscovery logic.
   - Default backend path from app-root/bin layout, retaining dev-friendly candidates where practical.
   - Default backend log file to the data dir.

4. Repository data layout:
   - Move contest rule YAMLs to `data/contest-rules/` to match the new runtime convention.

5. Documentation:
   - Update README/manual path examples and CLI option tables.
   - Remove references to DB in cwd and `--contest-rules-dir`.

## Expected files to change

- `Cargo.toml`
- `Cargo.lock`
- `backend/Cargo.toml`
- `backend/src/main.rs`
- `backend/src/db.rs`
- `launcher/Cargo.toml`
- `launcher/src/main.rs`
- `README.md`
- `docs/manual.md`
- `docs/manual-regeneration-prompt.md`
- New: `paths/Cargo.toml`
- New: `paths/src/lib.rs`
- Move:
  - `contest-rules/arrl-field-day.yaml` → `data/contest-rules/arrl-field-day.yaml`
  - `contest-rules/sc-qso-party.yaml` → `data/contest-rules/sc-qso-party.yaml`

## Risks / alternatives

- Existing launcher settings in `~/.config/log73-launcher` will no longer be read unless migration logic is added.
- Existing users relying on `--contest-rules-dir` will need to switch to `--data-dir`.
- Existing data under old/current working directories will not be migrated automatically.
- Fresh OS data dirs will need contest rules/data files installed or copied there.
- Alternative: keep `--contest-rules-dir` as a deprecated compatibility alias, but that conflicts slightly with “replace backend path related params”.

## Tests to run

- `cargo fmt --all`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets --all-features`
- `cargo run -p log73-backend -- --help`
