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

## Polish paragraph preservation and fallback status

1. Set the Polish model to `gpt-5.4-mini`, select the Slack preset, and save multiline custom instructions that request paragraph breaks at semantic boundaries while prohibiting implicit bullets.
2. Record explanatory speech with four roles: preface, current behavior, implication, and uncertainty/closing request.
3. Confirm the latest-text view and history show four prose paragraphs separated by one blank line.
4. Confirm the history metadata distinguishes `Polish適用`, `Polish・変更なし`, and `Rawフォールバック` from injection success/failure.
5. Copy the final text into Notepad and confirm the same blank lines remain.
6. Reinject the history item into Slack and confirm the same paragraph boundaries remain.
7. Restart KoeType and confirm the history still shows the same paragraph boundaries and Polish result state.
8. Run `Polish再実行` and repeat the history, copy, and reinjection checks.
9. Record a short single-topic utterance and confirm it remains one paragraph.
10. Confirm output contains only the rewritten body, with no explanation or confirmation message.

## Polish preset evaluation harness

1. From `src-tauri`, run `cargo run --example polish_preset_eval -- --list` and confirm every preset has at least two cases.
2. Run `cargo run --example polish_preset_eval -- --dry-run --output polish-preset-eval-results-dry-run.json` and confirm no credential prompt or network request occurs.
3. After explicit approval for API usage, run the 10-call baseline command documented in `docs/polish-preset-evaluation.md`.
4. Confirm the report contains model outputs and machine checks, but no API key or authorization header.
5. Fill the six `human` scores for every result and apply the documented case and preset gates.
6. Re-run only failing cases while tuning, then run one full regression suite after all targeted cases pass.

## Onboarding test recording

1. Open Settings.
2. Click `初回セットアップを再実行`.
3. Complete the test recording step.
4. Confirm recognized text appears in the onboarding preview.
5. Click `プレビューをコピー` and confirm the preview text is copied.
6. Confirm no text is pasted into the previously focused external app.
