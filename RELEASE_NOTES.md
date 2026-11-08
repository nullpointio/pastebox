# Pastebox v1.0.2 — version info in the exe

**A clipboard history that opens instantly, searches properly, and remembers nothing you didn't pin.**

Press `Win+Alt+V` → a quiet panel over whatever you're doing → type three letters → Enter → pasted.
Offline, no account, ~3 MB, no installer.

## What changed in 1.0.2

- The exe now carries a PE `VERSIONINFO` resource, so Task Manager's **Description** column and
  Explorer's Details tab show a product name and version instead of `Pastebox.ex…`.
- Fields: `CompanyName=nullpointio`, `ProductName=Pastebox`, `FileDescription=Clipboard history with
  instant search`, `FileVersion`/`ProductVersion=1.0.2.0`, `OriginalFilename=Pastebox.exe`,
  `LegalCopyright=MIT License — nullpointio`.
- Compiled into the same resource object as the icon; `-version` still reports the workspace version,
  which now matches. No behaviour change.

## Downloads

- `Pastebox.exe` — Windows 10/11 x64, portable, no runtime required.
- `Pastebox-v1.0.0-win-x64.zip` — the same exe, zipped.
- `SHA256SUMS.txt` — `powershell -c "(Get-FileHash .\Pastebox.exe -Algorithm SHA256).Hash"`

## ⚠️ Unsigned

SmartScreen will show "Windows protected your PC" on first run. That's expected — there is no
code-signing certificate. **More info → Run anyway**, or check the hash, or build from source.

## Why you'd replace Win+V

- **Search.** Win+V has none. Pastebox filters as you type and ranks by a match-then-recency score.
- **500 items, not 25.** Captures text, links, file lists and images.
- **Survives a restart** — but only if you pin it. Unpinned history is memory-only, by design.
- **One keystroke to paste** into the window you came from, without losing your place.

## Privacy

No network code in the binary. No telemetry, no sync, no update check. Unpinned history never touches
disk; the footer shows exactly how much is stored (usually `0 B`). Pins live in one plain-text file you
can read and delete.

## Known limitations

- Built and tested under Wine plus a Windows CI smoke test; **not yet run on a physical Windows
  machine**. If the panel doesn't appear on your setup, open an issue with your hotkey and Windows build.
- Plain text and file lists only — formatting is not preserved, images are pasted as images.
- If `Win+Alt+V` is taken, the app falls back to `Win+Shift+V`, then `Ctrl+Alt+V`, and says which one it
  registered on first run.
- No tray-library polish: the tray menu has Open / Pause / Clear unpinned / Quit.

## Verification

- 94 core tests (model, dedupe, search ranking, pin persistence, corrupt input) + 32 UI tests, on Linux.
- On `windows-latest`: full clippy, the same tests, plus `pastebox.exe -smoke` (registers the hotkey,
  round-trips the clipboard) and `-selftest` — the real binary, not just a compile.
- Size gate in CI: the release fails if the exe exceeds 10 MB.

MIT licensed.
