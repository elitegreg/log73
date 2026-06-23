# Log73 Keyboard Shortcuts

This page summarizes keyboard shortcuts and special key behavior in the logger UI.

## Global logger shortcuts

| Shortcut         | Action                                                    |
| ---------------- | --------------------------------------------------------- |
| `Ctrl+O`         | Prompt/change operator callsign                           |
| `F1`..`F12`      | Send message for active Run/S&P bank                      |
| `Esc`            | Stop sending / clear queue (or close CW text dialog)      |
| `PageUp`         | Increase CW WPM by 1                                      |
| `PageDown`       | Decrease CW WPM by 1                                      |
| `Ctrl+K`         | Open CW text dialog (CW/CW-R only)                        |
| `Alt+PageUp`     | Shift band up                                             |
| `Alt+PageDown`   | Shift band down                                           |
| `ArrowUp`        | Tune up by configured increment (Using RIT in Run Mode)   |
| `ArrowDown`      | Tune down by configured increment (Using RIT in Run Mode) |
| `Ctrl+ArrowUp`   | Jump to next band-map spot below current VFO              |
| `Ctrl+ArrowDown` | Jump to next band-map spot above current VFO              |
| `Alt+M`          | Mark current frequency                                    |
| `Alt+O`          | Store current spot                                        |
| `Alt+Q`          | Jump to last stored CQ frequency                          |
| `Ctrl+P`         | Spot It                                                   |

> Note: function keys and related global logger hotkeys are ignored while focus is inside the log table area.

## Callsign and exchange field behavior

- `Enter` in callsign with a numeric value (for example `14074`) sets radio frequency in kHz.
- `Enter` in callsign with a mode token (for example `CW`, `SSB`, `RTTY`, `FT8`) sets radio mode.
- `Enter` participates in ESM behavior when ESM is enabled.
- `Alt+Enter` in callsign/exchange fields logs immediately (normal validation still applies).
- `Tab` moves to the next empty editable field; if none are empty, it moves to the next editable field. Forward tab wraps within the log entry fields.
- `Shift+Tab` moves to the previous editable field and wraps within the log entry fields.

## CW text dialog (`Ctrl+K`)

| Key                       | Action                                |
| ------------------------- | ------------------------------------- |
| `Space`                   | Send current word with trailing space |
| `Enter`                   | Send current word and close dialog    |
| `Esc`                     | Close dialog                          |
| `Backspace` (empty input) | Ignored                               |

## Log window inline edit

| Key     | Action      |
| ------- | ----------- |
| `Enter` | Apply edit  |
| `Esc`   | Cancel edit |
