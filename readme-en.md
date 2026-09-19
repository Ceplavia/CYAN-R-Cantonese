# R-Cantonese

**[粵語](README.md)** | English

A Windows Cantonese (Jyutping) input method written in Rust, built on the Text Services Framework (TSF). Inspired by and feature-aligned with [jyutping-windows](https://github.com/yuetyam/jyutping-windows).

<p align="center">★ Special Thanks ★</p>

<p align="center"><a href="https://github.com/rime/rime-cantonese">rime-cantonese</a> | <a href="https://github.com/yuetyam/jyutping-windows">yuetyam/jyutping-windows</a></p>

<p align="center">★ Special Thanks ★</p>

## Features

- **Jyutping input** — type `nei hou` for「你好」; word input, multi-character input, automatic segmentation
- **Adaptive learning** — chosen candidates are remembered and ranked higher next time (`memory.sqlite3`)
- **Reverse lookup** — look up characters via other input methods:
  - `` ` `` (backtick) prefix — Mandarin pinyin reverse lookup
  - `v` prefix — Cangjie reverse lookup
  - `x` prefix — stroke reverse lookup
- **`/` symbol queries** — `/` opens the symbol table: `/digit` for full-width numeral variants, `/letter` for single-character Jyutping lookup + emoji
- **Customizable candidate window** — font sizes (label / number / comment), candidates per page, colors — all in the Settings Center
- **Character variants** — Traditional (Hong Kong) / Traditional (Taiwan) / Simplified candidates
- **Punctuation modes** — Chinese (full-width `，。？`) or English (ASCII `,.?`)
- **Half-width / full-width** — digit and letter width toggle
- **Settings Center** — graphical UI for every setting; Apply takes effect immediately

## Hotkeys

| Key | Action |
|-----|--------|
| `Ctrl` + `` ` `` | (default) open the options menu (fonts / punctuation / variants) |
| `Ctrl` + `.` | Toggle Chinese / English punctuation |
| `Shift` + `Space` | Toggle half-width / full-width |
| `Ctrl` + `Shift` + `Delete` | Remove weight of the currently selected word |

## Install

Run `r-cantonese-setup-x.x.x.exe`:

1. Choose an install path (default `C:\Program Files\R-Cantonese`)
2. The IME is registered under "Chinese (Hong Kong)" automatically
3. The Settings Center opens after install — tweak and hit Apply, or just close it (you can reopen it later by right-clicking the tray icon)
4. Press `Win + Space` and switch to R-Cantonese — ready to type

> Tip: the tray icon lives in the notification overflow ("^") — drag it out to keep it visible.

### Repository layout

| Directory | Role |
|-----------|------|
| `rcantonese/` | IME core (the TSF text service DLL) |
| `tray/` | Standalone tray process |
| `config-center/` | `config-center.exe` — settings UI |
| `preparing/` | Dictionary DB generator |

## License

CC0 1.0 Public Domain — use it however you like.
