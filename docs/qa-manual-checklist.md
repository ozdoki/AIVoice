# KoeType Manual QA Checklist

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

## Live transcript in FloatingBar

1. Open Settings.
2. Turn on `録音中にフローティングバーを表示する`.
3. Turn off `録音中の文字起こしをフローティングバーに表示する`, save, and record.
4. Confirm the FloatingBar shows waveform, state, and seconds only.
5. Turn on `録音中の文字起こしをフローティングバーに表示する`, save, and record with ASR model `gpt-realtime-whisper`.
6. Confirm in-progress transcript text appears in the FloatingBar while recording.
7. Stop recording and confirm final text is still injected into the target app.

## Launch at login

1. Open Settings.
2. Turn on `ログイン時にKoeTypeを起動する` and save.
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
3. Open KoeType and click `再注入` from the latest text or history.
4. Confirm focus returns to the target app and text is pasted there.
5. If no target was recorded, confirm KoeType shows a clear message asking to click the input target first.

## Onboarding test recording

1. Open Settings.
2. Click `初回セットアップを再実行`.
3. Complete the test recording step.
4. Confirm recognized text appears in the onboarding preview.
5. Click `プレビューをコピー` and confirm the preview text is copied.
6. Confirm no text is pasted into the previously focused external app.
