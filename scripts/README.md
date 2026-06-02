# Test data scripts

## SC QSO Party (In State)

`gen_sc_qso_party_in_state.py` generates contacts for `SC-QSO-PARTY (In State)` **through the backend REST API**.

It does **not** write SQLite directly; backend must be running.

### Requirements

- Python 3.10+
- `requests` (`pip install requests`)
- Backend running (default: `http://127.0.0.1:7300`)

### Example

```bash
python3 scripts/gen_sc_qso_party_in_state.py \
  --base-url http://127.0.0.1:7300 \
  --station-callsign N0CALL \
  --my-county RICH \
  --qso-count 5000 \
  --supercheckpartial data/MASTER.SCP \
  --start "2026-06-01 00:00:00" \
  --hours 24
```

### Optional filters/auth

```bash
python3 scripts/gen_sc_qso_party_in_state.py \
  --base-url http://127.0.0.1:7300 \
  --station-callsign N0CALL \
  --my-county RICH \
  --qso-count 1000 \
  --supercheckpartial data/MASTER.SCP \
  --start 1767225600 \
  --hours 12 \
  --callsign-regex '^(K|N|W|VA|VE)' \
  --username admin
```

### Notes

- Callsigns are filtered to US/Canada-like prefixes: `K/N/W/AA-AL/VA/VE`.
- RST is always `599` sent/received.
- `SRX_STRING` behavior:
  - `VA/VE` stations send province code.
  - `K/N/W/AA-AL` stations send mostly state/province, occasionally SC county.
- Exchange is kept stable per callsign.
- Dupes are allowed but capped at ~2%.
