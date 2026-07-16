# Security Policy

## Supported scope

KoeType is an early-stage Windows-first desktop app. Security review currently focuses on:

- API key handling and local secret storage
- Tauri command exposure and desktop permissions
- Local history, dictionary, snippet, recovery, and settings storage
- OpenAI-compatible API request handling
- Dependency vulnerabilities in Rust, Tauri, React, and Vite

## Reporting a vulnerability

Please do not post API keys, tokens, private logs, exploit details, or personal data in public issues.

If GitHub private vulnerability reporting is available for this repository, use it first. Otherwise, open a GitHub issue with a short, non-sensitive summary and request a private follow-up channel.

Include:

- Affected version or commit
- Operating system and app build mode
- Impact summary
- Minimal reproduction steps without secrets
- Whether the issue may expose API keys, local user data, audio, or transcript history

## Local data and secrets

KoeType is designed for BYOK usage. OpenAI API keys should be stored through the app UI and saved in Windows Credential Manager. API keys must not be committed to the repository, written to logs, or included in screenshots or issue reports.

Local history, dictionary, snippets, usage summaries, and recovery data may contain private user content. Treat those files as sensitive user data.

Audio transcription and Polish are performed by the configured OpenAI-compatible API. The settings UI displays only the destination host, never URL credentials, paths, queries, API keys, transcript bodies, dictionary contents, or actual window titles. Dictionary terms and foreground app metadata may be included in Batch ASR or Polish prompts when enabled. Recovery audio is retained locally after failed or interrupted sessions and removed after successful completion.

Selected voice edit is an explicit exception to the normal foreground-context rule: when the user invokes its hotkey, the selected source text and transcribed voice instruction are sent to the configured compatible API to generate an edit proposal. The selected source text is held in memory only, is not written to history or recovery metadata, is scrubbed when a pending preview expires after ten minutes, and must never be logged. History/recovery may retain only the voice instruction and proposal. Replacement requires UI Automation to recapture the same window handle, process ID, and selected text; clipboard-fallback capture is Copy-only. UI Automation does not provide this implementation with a stable range/control identity, so a same-window selection changed to identical text during the final check remains a residual race. Replacement uses the shared clipboard-paste path: it sets the proposal as Unicode text, verifies its owner/sequence and exact text before `SendInput`, sends Ctrl+V, and leaves the proposal on the clipboard. Output injection does not snapshot or restore the previous clipboard, so non-HGLOBAL/private formats do not block it. The Ctrl+C selection-capture fallback alone snapshots and restores a known matching copy sequence. A partially sent Ctrl+V is not retried and leaves the full proposal on the clipboard for recovery. Focus is checked immediately before and after SendInput, but a target change between the pre-check and SendInput remains a residual TOCTOU race; once all four Ctrl+V events are sent, a later focus change is treated as uncertain success to prevent duplicate replacement and is surfaced without transcript content.

## Dependency review

Before releases, maintainers should run:

```powershell
cmd /c "corepack pnpm audit"
Set-Location .\src-tauri
cargo audit
```

If `cargo audit` is not installed, install it outside the repository or through a local tool cache.
