# Pastebox — searchable Windows clipboard history, offline and account-free

**A Windows 10/11 clipboard history that opens instantly with `Win+Alt+V`, searches text, images, and files as you type, and remembers nothing you didn't pin. No account, no cloud, nothing on disk unless you pin it.**

Press `Win+Alt+V`. A quiet panel opens over whatever you're doing with everything you've copied —
text, links, images, files — newest first. Type three letters, hit Enter, and it's pasted where you
were. Nothing leaves your machine, and nothing is written to disk unless you pin it.

![Pastebox](docs/assets/screenshot.png)

## Download

**[Pastebox.exe — latest release](https://github.com/nullpointio/pastebox/releases/latest)**

- Windows 10/11, x64. One portable exe (~3 MB), no installer, no runtime to install.
- The exe is **unsigned**: SmartScreen will warn on first run (`More info → Run anyway`). SHA-256 hashes
  are on the release page.

## Why not Windows' own Win+V?

Windows keeps 25 items, has no search, wipes the list on restart, drops anything over 4 MB, and never
records files at all. Pastebox keeps 500 items, searches them as you type, captures files and images,
and lets you pin the ten things you paste all day.

## Common questions

**How do I search my Windows clipboard history?** Press `Win+Alt+V` to open Pastebox, then type any part of the text, file name, or image you copied. The list filters as you type; `Enter` pastes the selected item into the window you were in.

**Does Pastebox keep clipboard history after a restart?** Only pinned items survive a restart. Unpinned text, images, and files live in memory and disappear when Pastebox exits; the footer shows `0 B` on disk unless you have pins.

## Privacy — read this before you install it

A clipboard manager sees everything you copy, including passwords. Pastebox is built around that:

- **No network code in the binary.** No sync, no telemetry, no update check, no crash reporting.
- **Nothing is written to disk unless you pin it.** Unpinned history lives in memory and dies with the
  process. The footer shows how much is on disk, which is `0 B` unless you have pins.
- **Pinned items only** are saved, to a single plain-text file you can read and delete.

If you copy from a password manager, most already suppress clipboard history on their side — and
whenever the clipboard carries the `ExcludeClipboardContentFromMonitorProcessing` format, Pastebox
skips that item entirely, whoever set it.

## Use

| Key | Action |
|---|---|
| `Win+Alt+V` | open / hide the panel (falls back to `Win+Shift+V`, then `Ctrl+Alt+V` if taken) |
| type | filter as you type |
| `↑` `↓` | move selection |
| `Enter` | paste into the window you were in |
| `Ctrl+P` | pin / unpin |
| `Del` | remove the row |
| `Esc` | close |

## What it is not

No cloud sync, no accounts, no mobile app, no OCR of images, no rich-text preservation (plain text and
file lists only). It is not a password manager.

## Build

Native Rust, no runtime:

```bash
cargo test -p pastebox-core -p pastebox-ui --release
cargo build --release -p pastebox-app        # on Windows
# cross-compile from Linux (MinGW):
cargo build --release --target x86_64-pc-windows-gnu -p pastebox-app
```

`pastebox-core` is pure logic (model, dedupe, search ranking, pins) with 94 tests and no platform code.
`pastebox-ui` is the egui surface. `pastebox-platform` is the Win32 layer: clipboard listener, global
hotkey, paste-back, tray. `pastebox-app` wires them together and exposes `-smoke` / `-selftest` so CI
runs the actual exe on Windows before any release.

## Memory

The history is in-memory by design, so it is also bounded by design:

- **Images are stored encoded.** A clipboard image arrives as a raw DIB (~4 bytes per pixel — 33 MB
  for a 4K screenshot). It is converted to PNG once, on capture, together with a ≤96 px thumbnail;
  the raw block is freed immediately. Rows draw the thumbnail, and the full pixels are decoded only
  for the row being previewed or pasted.
- **Per-item caps.** Text is capped at 1 MB, an image's encoded payload at 8 MB (over that, the row
  and thumbnail stay, the pasteable pixels go).
- **Byte budget.** Beyond the 500-item cap the history also holds at most 48 MB of payload; the
  oldest unpinned rows are evicted first.
- **Textures are viewport-scoped.** Row thumbnails live on the GPU only while their row is on screen,
  and exactly one full-size preview texture exists at a time.

`pastebox -mem` runs a scripted session (60 screenshots, 40 text clips, 20 panel opens) and prints the
peak working set, so memory claims can be measured instead of estimated:

```bash
pastebox -mem
# clips=100 images=60 retained=1.9 MB
# encoded 60 screenshots: avg 10 KB each from 296.6 MB of DIBs (7.58 s encode)
# steady RSS 23.6 MB
# peak RSS 28.8 MB (before the session 21.3 MB), scripted run 10.60 s
```

The same script against v1.0.2's pipeline peaks at **56.0 MB** and keeps **29.7 MB** of raw DIBs.

## License

MIT — see [LICENSE](LICENSE).
