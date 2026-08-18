# Contest rule schema

Log73 contest rules use schema version 2. Every `.yaml` or `.yml` file in a contest-rules directory must begin with `schema: 2`. A file may contribute any combination of `value_sets`, `presets`, `profiles`, and `contests`; the bundled configuration separates these into catalog files and contest files.

```yaml
schema: 2

value_sets:
  states:
    values_from_file: us_states.dat

presets:
  rst-received:
    label: RST(r)
    type: RST
    adif: RST_RCVD
    direction: received

profiles:
  hf-cw:
    bands: [160, 80, 40, 20, 15, 10]
    modes: [CW]

contests:
  EXAMPLE-CW:
    name: Example Contest (CW)
    profiles: [hf-cw]
    exchange:
      - { id: rst-received, preset: rst-received }
    scoring:
      qso_points: { points: 1 }
      dupe_key: [CALL, BAND]
```

## Catalog composition

Catalog names are global across all loaded YAML files and must be unique.

- A value set defines `values`, `values_from_file`, or `use`. `exclude` removes values from the result. A `.dat` file contains one value per line; blank lines and lines beginning with `#` are ignored. File references must be plain filenames within a loaded contest-rules directory.
- A preset is a reusable mapping fragment. Add `preset: <name>` to a field or another mapping and specify only its local additions or overrides. Presets may reference other presets.
- A profile is an overlay-only rule fragment. It may compose other profiles with `profiles`, but it does not take parameters.
- A contest may compose profiles and may inherit another contest with `extends`. Profiles are applied from left to right, followed by the inherited contest and then the contest's own values.

Reference cycles, missing references, duplicate catalog names, and unsupported schema versions stop startup with an error.

## Keyed collection merging

Ordered object lists use stable `id` values. When both inherited and overriding values are keyed lists, matching IDs merge recursively, new IDs append, and unchanged items retain their order.

```yaml
contests:
  PHONE-VARIANT:
    extends: CW-VARIANT
    modes: [SSB]
    exchange:
      - id: rst-sent
        default: 59
      - id: cw-only-field
        remove: true
      - id: phone-field
        preset: rst-received
        after: rst-sent
```

`remove: true` deletes an inherited item. `before` and `after` place a new or patched item relative to another ID. An empty list clears an inherited list. Lists without stable IDs replace the inherited list as a whole. Duplicate IDs and mixed keyed/unkeyed lists are rejected.

## Contest fields

The main contest keys are:

- `name`, `bands`, `modes`, and optional `excluded_modes`
- optional contest-local `value_sets`
- `setup_fields` for all values collected when a log is created or edited
- `exchange` for sent and received QSO fields
- `scoring`
- optional `cabrillo` and `metadata`
- optional `qso_table`

Setup and Cabrillo export fields have an `id`, storage `key`, display `label`, and `type`. They may also define `required`, `regex`, `valid_values`, `in_sets`, `valid_values_or_regex`, `default`, `widget`, `help_text`, `max_lines`, and `preserve_case`. A setup field with `cabrillo_header` is emitted under that header during Cabrillo export. `multi_single_has_mult_transmitter` marks the category-transmitter field for contests where multi-single contacts carry run/mult transmitter IDs.

Exchange fields have an `id`, `label`, `type`, ADIF field name in `adif`, and `direction: sent|received`. Sent fields may use `source` to read a setup value and `fixed: true` to prevent editing. Serial fields support `serial_scope: global|band|category_transmitter`. `only_when` conditionally enables a field. `table: show|hide|auto` overrides its derived table visibility.

Supported authored field types are `String`, `RST`, `Numeric`, and `Serial`, optionally followed by a maximum length such as `String:16` or `Serial:4`. The resolved API represents these as an `input` object and groups constraints into a `validation` object.

## Scoring and Cabrillo

Scoring settings live under `scoring`: `qso_points`, `dupe_key`, `multipliers`, `bonus_points`, `param_multipliers`, and `multiplier_count_bonus_points`. Scoring rule lists use stable IDs. Conditions may use direct `values`, `in_set`/`in_sets`, `exclude_values`/`exclude_in_sets`, `matches_field` for a case-insensitive comparison to another QSO field, and callsign suffix filters. References to value sets are expanded before rules reach scoring or the API.

`modes` accepts exact ADIF mode names as well as `PHONE` (SSB, FM, or AM) and `DIGITAL` (any non-CW, non-phone mode). `excluded_modes` uses the same matching rules and is applied after the allowed list. This supports contests that permit all digital modes except RTTY.

`qso_points.grid_distance` scores a contact from two Maidenhead locators. It names the station and contacted grid fields and defines `base_points`, `kilometers_per_point`, and `minimum_distance_points`. The scorer uses the centers of the first four locator characters and great-circle distance, then awards `base_points + max(minimum_distance_points, ceil(distance / kilometers_per_point))`.

Cabrillo configuration uses `fixed_headers` and `export_fields`. All fixed headers and export fields use stable IDs. The Cabrillo `CONTEST` header is the first whitespace-delimited word of the contest rule ID, so a rule such as `SC-QSO-PARTY (In State)` exports `SC-QSO-PARTY`. Category and other log-specific headers belong in `setup_fields` with `cabrillo_header`, so there is only one setup-field list.

## Derived QSO table

When `qso_table` is omitted, the loader creates typed columns for UTC date/time, frequency, mode, callsign, exchange fields, scoring totals, and operator. Received and mutable sent exchange fields are included. Fixed sent values are omitted. Generated sent serials remain visible but read-only. `table: show` or `table: hide` changes an exchange field's default treatment.

For a fully custom table, provide `qso_table.columns`. Each column requires a stable `id`, `label`, `source: adif|meta`, `field`, and `editable`; `format` may be `text`, `date_time_utc`, or `frequency_khz`.
