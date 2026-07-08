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

## Dependency review

Before releases, maintainers should run:

```powershell
cmd /c "corepack pnpm audit"
Set-Location .\src-tauri
cargo audit
```

If `cargo audit` is not installed, install it outside the repository or through a local tool cache.
