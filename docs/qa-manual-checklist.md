# AIVoice Manual QA Checklist

## Realtime fallback

Use this only for local QA.

1. Close AIVoice.
2. Start it with `AIVOICE_FORCE_REALTIME_FAIL=1`.
3. Confirm ASR model is `gpt-realtime-whisper` and API key is configured.
4. Focus Notepad or another text field.
5. Record a short utterance with the normal hotkey.
6. Stop recording.
7. Confirm final text is injected via batch ASR, or a recovery/error path is shown without freezing.

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
5. Confirm no text is pasted into the previously focused external app.
