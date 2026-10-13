# Shared domain fixtures

This directory holds JSON fixtures executed directly by both the Rust backend
and the JS frontend. `callsigns.json` is the first; later fixtures follow the
same convention.

## Parity rule

Every consumer iterates **every case** and asserts **its own fields** with no
filtering or per-language skips. Adding a case automatically extends all
harnesses. If a behavior genuinely differs by language, that is a contract bug:
fix the implementations, not the fixture.

## `callsigns.json` format

```json
{
  "version": 1,
  "dxcc_db": { "entities": [...], "rules": [...] },
  "cases": [
    {
      "input": "DL/W1AW",
      "logging_valid": true,
      "query_valid": true,
      "base": "W1AW",
      "modifier": "DL",
      "dupe_identity": "W1AW",
      "wpx_prefix": "DL0",
      "dxcc": { "adif": 230, "primary_prefix": "DL" }
    }
  ]
}
```

- `version`: fixture schema version. Harnesses reject anything but `1`.
- `dxcc_db`: minimal `entities`/`rules` database in the exact shape both
  `DxccDatabase` (Rust) and `lookupDxcc` (JS) consume, so DXCC expectations run
  without a CTY parser in JS.
- `cases[]`: every field is required on every case.
  - `input`: raw input, possibly with whitespace, lowercase, or `?` markers.
  - `logging_valid` / `query_valid`: strict-logging vs wildcard-query validity.
  - `base` / `modifier`: resolved station identity and operating component
    (`null` when slash structure is broken or neither component is
    callsign-like). Note `base` is still the whole input for structurally
    sound but digitless singles such as `KABC`.
  - `dupe_identity`: always a string; the resolved base when available, else
    the normalized full input (deterministic fallback for legacy values).
  - `wpx_prefix`: WPX prefix extraction result, or `null` when none derives.
  - `dxcc`: `{ adif, primary_prefix }` from lookup against `dxcc_db`, or
    `null` when nothing resolves.

`wpx_prefix` and `dxcc` are pure-function outputs and intentionally independent
of validity: an over-long or digitless input can still carry a WPX prefix or a
DXCC result, and the fixture locks that. DXCC expectations lock the lookup
algorithm's output against the embedded database (operating-entity selection
and exact-match precedence); they are not real-world DXCC truth. In particular
`KL7RA/WK9` and `EA8/K1A` resolve to the right-side `K` rule first, which is
the preserved lookup order, not a geography claim.

## Consumers

Rust (via `include_str!` + `serde_json`, cases shared through
`crate::callsign::load_fixture`):

- `backend/src/callsign.rs` tests: `base`, `modifier`, `dupe_identity`.
- `backend/src/validation.rs` tests: `logging_valid`, `query_valid`.
- `backend/src/scoring.rs` tests: `dupe_identity` through scoring's normalizer.
- `backend/src/dxcc.rs` tests: `wpx_prefix`, `dxcc` against `dxcc_db`.

JS (via `loadFixture.js` in this directory):

- `src/domain/callsign.test.js`: `base`, `modifier`, `dupe_identity`.
- `src/domain/validation.test.js`: `logging_valid`, `query_valid`.
- `src/domain/dupes.test.js`: `dupe_identity`.
- `src/domain/dxcc.test.js`: `wpx_prefix`, `dxcc` against `dxcc_db`.
