#!/usr/bin/env python3
"""Generate SC-QSO-PARTY (In State) contacts via backend REST API.

Requires backend running; does not touch SQLite directly.
"""

from __future__ import annotations

import argparse
import datetime as dt
import getpass
import random
import re
import sys
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Sequence, Tuple

try:
    import requests
except ImportError as exc:  # pragma: no cover
    raise SystemExit("This script requires 'requests'. Install with: pip install requests") from exc

CONTEST_ID = "SC-QSO-PARTY (In State)"
US_CA_CALL_RE = re.compile(r"^(?:[KNW]|A[A-L]|VA|VE)[A-Z0-9/]*$")
CANADIAN_PROVINCES = {
    "AB",
    "BC",
    "MB",
    "NB",
    "NL",
    "NS",
    "NT",
    "NU",
    "ON",
    "PE",
    "QC",
    "SK",
    "YT",
}
FREQ_RANGES_HZ = {
    160: (1_800_000, 2_000_000),
    80: (3_500_000, 4_000_000),
    60: (5_330_500, 5_406_500),
    40: (7_000_000, 7_300_000),
    30: (10_100_000, 10_150_000),
    20: (14_000_000, 14_350_000),
    17: (18_068_000, 18_168_000),
    15: (21_000_000, 21_450_000),
    12: (24_890_000, 24_990_000),
    10: (28_000_000, 29_700_000),
    6: (50_000_000, 54_000_000),
    2: (144_000_000, 148_000_000),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate SC-QSO-PARTY (In State) contacts through REST API"
    )
    parser.add_argument("--base-url", required=True, help="Backend URL, e.g. http://127.0.0.1:7300")
    parser.add_argument("--station-callsign", required=True)
    parser.add_argument("--my-county", required=True, help="Your 4-char SC county code (e.g. RICH)")
    parser.add_argument("--qso-count", required=True, type=int)
    parser.add_argument("--supercheckpartial", required=True, type=Path, help="Path to MASTER.SCP")
    parser.add_argument(
        "--start",
        required=True,
        help="Start timestamp: epoch seconds, RFC3339, or 'YYYY-MM-DD HH:MM:SS'",
    )
    parser.add_argument("--hours", required=True, type=float)
    parser.add_argument("--callsign-regex", help="Optional regex filter applied after US/CA prefix filter")
    parser.add_argument("--username", help="HTTP Basic Auth username")
    parser.add_argument("--password", help="HTTP Basic Auth password (omit to prompt)")
    parser.add_argument("--batch-size", type=int, default=100, help="Contact POST batch size (<=100)")
    return parser.parse_args()


def parse_start_epoch(value: str) -> int:
    value = value.strip()
    if re.fullmatch(r"-?\d+", value):
        return int(value)

    try:
        parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=dt.timezone.utc)
        return int(parsed.timestamp())
    except ValueError:
        pass

    try:
        parsed = dt.datetime.strptime(value, "%Y-%m-%d %H:%M:%S").replace(tzinfo=dt.timezone.utc)
        return int(parsed.timestamp())
    except ValueError as exc:
        raise ValueError("Unable to parse --start timestamp") from exc


def make_session(args: argparse.Namespace) -> requests.Session:
    session = requests.Session()
    if args.username:
        password = args.password if args.password is not None else getpass.getpass("Password: ")
        session.auth = (args.username, password)
    return session


def api_json(
    session: requests.Session,
    base_url: str,
    method: str,
    path: str,
    *,
    params: Optional[dict] = None,
    payload: Optional[dict | list] = None,
) -> dict | list:
    url = f"{base_url.rstrip('/')}{path}"
    response = session.request(method, url, params=params, json=payload, timeout=30)
    response.raise_for_status()
    return response.json()


def load_callsigns(path: Path, pattern: Optional[re.Pattern[str]]) -> List[str]:
    if not path.exists():
        raise FileNotFoundError(f"SCP file not found: {path}")

    callsigns: List[str] = []
    for raw_line in path.read_text(encoding="utf-8", errors="ignore").splitlines():
        line = raw_line.strip().upper()
        if not line or line.startswith("#") or line.startswith("!"):
            continue
        if not US_CA_CALL_RE.match(line):
            continue
        if pattern and not pattern.search(line):
            continue
        callsigns.append(line)

    # preserve order while deduping
    unique = list(dict.fromkeys(callsigns))
    if not unique:
        raise ValueError("No eligible callsigns after filtering")
    return unique


def classify_srx_values(contest_settings: dict) -> Tuple[List[str], List[str], List[str]]:
    exchange_fields = contest_settings.get("exchange") or []
    srx_field = next((field for field in exchange_fields if field.get("adif") == "SRX_STRING"), None)
    if not srx_field:
        raise ValueError("Contest settings missing SRX_STRING field")

    valid_values = [str(value).upper() for value in (srx_field.get("valid_values") or [])]
    counties = sorted([value for value in valid_values if len(value) == 4])
    two_char = sorted([value for value in valid_values if len(value) == 2 and value != "DX"])
    provinces = sorted([value for value in two_char if value in CANADIAN_PROVINCES])
    states = sorted([value for value in two_char if value not in CANADIAN_PROVINCES])

    if not counties:
        raise ValueError("Unable to derive county list from contest settings")
    if not states:
        raise ValueError("Unable to derive state list from contest settings")
    if not provinces:
        raise ValueError("Unable to derive province list from contest settings")

    return counties, states, provinces


def validate_my_county(contest_settings: dict, my_county: str) -> str:
    normalized = my_county.strip().upper()
    log_params = contest_settings.get("log_params") or []
    county_param = next((field for field in log_params if field.get("name") == "County"), None)
    valid_values = [str(value).upper() for value in (county_param or {}).get("valid_values", [])]
    if valid_values and normalized not in valid_values:
        raise ValueError(f"--my-county {normalized} is not valid for {CONTEST_ID}")
    return normalized


def random_freq_for_band(meters: int) -> int:
    low, high = FREQ_RANGES_HZ.get(meters, FREQ_RANGES_HZ[20])
    return random.randint(low, high)


def random_epoch(start_epoch: int, end_epoch: int) -> int:
    if end_epoch <= start_epoch:
        return start_epoch
    return random.randint(start_epoch, end_epoch)


def is_us_callsign(callsign: str) -> bool:
    return callsign.startswith(("K", "N", "W")) or bool(re.match(r"^A[A-L]", callsign))


def choose_exchange_for_callsign(
    callsign: str,
    counties: Sequence[str],
    states: Sequence[str],
    provinces: Sequence[str],
) -> str:
    if callsign.startswith(("VA", "VE")):
        return random.choice(list(provinces))

    # US callsigns: mostly states, occasionally SC county
    if random.random() < 0.20:
        return random.choice(list(counties))
    return random.choice(list(states) + list(provinces))


def generate_contacts(
    *,
    qso_count: int,
    callsigns: Sequence[str],
    station_callsign: str,
    my_county: str,
    start_epoch: int,
    end_epoch: int,
    allowed_bands: Sequence[int],
    allowed_modes: Sequence[str],
    counties: Sequence[str],
    states: Sequence[str],
    provinces: Sequence[str],
) -> List[dict]:
    if not allowed_bands or not allowed_modes:
        raise ValueError("Contest settings must include allowed bands and modes")

    station_callsign = station_callsign.strip().upper()
    band_mode_pairs = [(band, mode) for band in allowed_bands for mode in allowed_modes]
    dupe_limit = int(qso_count * 0.02)

    contacts: List[dict] = []
    per_call_exchange: Dict[str, str] = {}
    per_call_used_pairs: Dict[str, set[Tuple[int, str]]] = {}
    seen_signatures: set[Tuple[str, int, str, str, str]] = set()
    dupes = 0

    max_attempts = max(5_000, qso_count * 200)
    attempts = 0
    while len(contacts) < qso_count and attempts < max_attempts:
        attempts += 1

        call = random.choice(callsigns)
        srx = per_call_exchange.setdefault(
            call, choose_exchange_for_callsign(call, counties, states, provinces)
        )

        used_pairs = per_call_used_pairs.setdefault(call, set())
        available_pairs = [pair for pair in band_mode_pairs if pair not in used_pairs]

        if available_pairs:
            band, mode = random.choice(available_pairs)
        else:
            band, mode = random.choice(band_mode_pairs)

        signature = (call, band, mode, my_county, srx)
        is_dupe = signature in seen_signatures
        if is_dupe and dupes >= dupe_limit:
            continue

        if is_dupe:
            dupes += 1
        else:
            seen_signatures.add(signature)
            used_pairs.add((band, mode))

        contact = {
            "QSO_DATE_TIME_ON": random_epoch(start_epoch, end_epoch),
            "STATION_CALLSIGN": station_callsign,
            "OPERATOR": station_callsign,
            "CONTEST_ID": CONTEST_ID,
            "CALL": call,
            "BAND": f"{band}m",
            "FREQ": random_freq_for_band(band),
            "MODE": mode,
            "RST_SENT": 599,
            "RST_RCVD": 599,
            "STX_STRING": my_county,
            "SRX_STRING": srx,
        }
        contacts.append(contact)

    if len(contacts) < qso_count:
        raise RuntimeError(
            f"Unable to generate enough contacts ({len(contacts)}/{qso_count}); "
            "try a larger callsign pool or lower qso-count"
        )

    return contacts


def chunked(values: Sequence[dict], size: int) -> Iterable[List[dict]]:
    for i in range(0, len(values), size):
        yield list(values[i : i + size])


def fallback_param_value(field: dict) -> str | int:
    field_type = str(field.get("type") or "STRING:8").strip().upper()
    if field_type.startswith("NUMERIC"):
        return 1
    if field_type.startswith("RST"):
        return "599"
    return "TEST"


def build_contest_params(contest_settings: dict, my_county: str) -> dict:
    params: dict = {"County": my_county}

    persisted_fields = list(contest_settings.get("log_params") or [])
    cabrillo = contest_settings.get("cabrillo") or {}
    persisted_fields.extend(cabrillo.get("log_fields") or [])

    for field in persisted_fields:
        name = field.get("name")
        if not name or name in params:
            continue

        required = field.get("required", True)
        if required is False:
            continue

        if "default" in field and field.get("default") is not None:
            params[name] = field["default"]
            continue

        valid_values = field.get("valid_values") or []
        if valid_values:
            params[name] = valid_values[0]
            continue

        params[name] = fallback_param_value(field)

    return params


def create_log(
    session: requests.Session,
    base_url: str,
    station_callsign: str,
    contest_settings: dict,
    my_county: str,
    start_epoch: int,
) -> int:
    stamp = dt.datetime.fromtimestamp(start_epoch, tz=dt.timezone.utc)
    name = f"Test {stamp:%Y-%m-%d} {stamp:%H%M%S}"
    payload = {
        "name": name,
        "contest_id": CONTEST_ID,
        "station_callsign": station_callsign.strip().upper(),
        "contest_params": build_contest_params(contest_settings, my_county),
    }
    body = api_json(session, base_url, "POST", "/api/logs", payload=payload)
    if not body.get("ok"):
        raise RuntimeError(f"Log creation failed: {body.get('error', 'unknown error')}")
    log = body.get("log") or {}
    log_id = log.get("id")
    if not isinstance(log_id, int):
        raise RuntimeError("Log creation succeeded but response missing log.id")
    return log_id


def post_contacts(session: requests.Session, base_url: str, log_id: int, contacts: Sequence[dict], batch_size: int) -> None:
    if batch_size < 1 or batch_size > 100:
        raise ValueError("--batch-size must be between 1 and 100")

    for batch in chunked(list(contacts), batch_size):
        body = api_json(session, base_url, "POST", f"/api/logs/{log_id}/contacts", payload=batch)
        if not body.get("ok"):
            raise RuntimeError(f"Contact upload failed: {body.get('error', 'unknown error')}")


def main() -> int:
    args = parse_args()
    if args.qso_count <= 0:
        raise SystemExit("--qso-count must be positive")
    if args.hours <= 0:
        raise SystemExit("--hours must be positive")

    try:
        start_epoch = parse_start_epoch(args.start)
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc
    end_epoch = start_epoch + int(round(args.hours * 3600))

    extra_pattern = re.compile(args.callsign_regex) if args.callsign_regex else None
    callsigns = load_callsigns(args.supercheckpartial, extra_pattern)

    session = make_session(args)

    try:
        contest_settings = api_json(
            session,
            args.base_url,
            "GET",
            "/api/contest-settings",
            params={"contest_id": CONTEST_ID},
        )
    except requests.RequestException as exc:
        raise SystemExit(f"Unable to reach backend API at {args.base_url}: {exc}") from exc

    my_county = validate_my_county(contest_settings, args.my_county)
    counties, states, provinces = classify_srx_values(contest_settings)
    allowed_bands = [int(band) for band in (contest_settings.get("allowed_bands") or [])]
    allowed_modes = [
        str(mode).strip().upper()
        for mode in (contest_settings.get("allowed_modes") or [])
        if str(mode).strip().upper() == "CW"
    ]
    if not allowed_modes:
        raise SystemExit("Contest does not allow CW mode; cannot use fixed RST=599")

    contacts = generate_contacts(
        qso_count=args.qso_count,
        callsigns=callsigns,
        station_callsign=args.station_callsign,
        my_county=my_county,
        start_epoch=start_epoch,
        end_epoch=end_epoch,
        allowed_bands=allowed_bands,
        allowed_modes=allowed_modes,
        counties=counties,
        states=states,
        provinces=provinces,
    )

    try:
        log_id = create_log(
            session,
            args.base_url,
            args.station_callsign,
            contest_settings,
            my_county,
            start_epoch,
        )
        post_contacts(session, args.base_url, log_id, contacts, args.batch_size)
    except requests.RequestException as exc:
        raise SystemExit(f"API request failed: {exc}") from exc
    except RuntimeError as exc:
        raise SystemExit(str(exc)) from exc

    print(
        f"Created log {log_id} ({CONTEST_ID}) with {len(contacts)} contacts via {args.base_url.rstrip('/')}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
