# Contributing to KoeType

Thanks for your interest in KoeType. This project is early-stage and currently prioritizes a small, stable Windows-first voice input workflow.

## Project direction

KoeType focuses on:

- Local-first desktop usage
- BYOK OpenAI-compatible speech recognition and text polishing
- Safe API key handling through Windows Credential Manager
- Reliable hotkeys, recording, recovery, and text injection
- Explicit F9 selected-text voice editing with mandatory preview and safe replacement
- Clear boundaries between Windows-specific native code and the React UI

Out of scope for now:

- Team accounts or shared dictionaries
- SSO, admin consoles, or billing
- Cloud sync
- Passive selection monitoring, whole-document editing, or editing without an explicit user hotkey
- macOS support

## Development setup

```powershell
npm install
npm run build
Set-Location .\src-tauri
cargo check
```

Use the app settings screen to save an OpenAI API key. Do not commit `.env` files or credentials.

## Pull requests

Keep pull requests focused and small. Include:

- What changed
- Why it changed
- How it was tested
- Any user-data, secret-handling, or Tauri-permission impact

Before opening a pull request, run the relevant checks:

```powershell
cmd /c ".\node_modules\.bin\tsc.cmd --noEmit"
cmd /c ".\node_modules\.bin\vite.cmd build"
Set-Location .\src-tauri
cargo fmt --check
cargo check
cargo test --no-run
```

For security-sensitive changes, also run dependency audits when possible:

```powershell
cmd /c "corepack pnpm audit"
cargo audit
```
