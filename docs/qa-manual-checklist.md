# AIVoice Manual QA Checklist

## Realtime fallback

Use this only for local QA.

1. Run the fallback QA launcher:

```powershell
corepack pnpm run app:restart:fallback-qa
```

2. Confirm ASR model is `gpt-realtime-whisper` and API key is configured.
3. Focus Notepad or another text field.
4. Record a short utterance with the normal hotkey.
5. Stop recording.
6. Confirm final text is injected via batch ASR, or a recovery/error path is shown without freezing.
7. When finished, return to the normal app:

```powershell
corepack pnpm run app:restart
```

## Launch at login

1. Open Settings.
2. Turn on `ログイン時にAIVoiceを起動する` and save.
3. Confirm `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\AIVoice` exists.
4. Turn it off and save.
5. Confirm the same Run value is removed.

PowerShell read-only check:

```powershell
Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name AIVoice -ErrorAction SilentlyContinue
```

## Reinject target restore

1. Focus Notepad or another target app.
2. Type or click in the target input area.
3. Open AIVoice and click `再注入` from the latest text or history.
4. Confirm focus returns to the target app and text is pasted there.
5. If no target was recorded, confirm AIVoice shows a clear message asking to click the input target first.

## Onboarding test recording

1. Open Settings.
2. Click `初回セットアップを再実行`.
3. Complete the test recording step.
4. Confirm recognized text appears in the onboarding preview.
5. Click `プレビューをコピー` and confirm the preview text is copied.
6. Confirm no text is pasted into the previously focused external app.
