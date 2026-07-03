# Polish Preset Evaluation

This fixture is for prompt quality review without calling the live API.

## Shared input

```text
えっと明日の朝までにAIVoiceのリアルタイムASRのフォールバック確認をして、もしgpt-realtime-whisperが失敗したらbatchで入るか見て、あとCursorの履歴から辞書追加するとき何が追加されるか分かるようにして、終わったらGitHubのチケットに進捗を書いてください。
```

## Rubric

- Meaning preserved: no invented facts, names, deadlines, or completion claims.
- Filler removed: hesitation and repeated starts are removed.
- Preset fit: output shape clearly matches Slack, email, memo, AI prompt, or technical note.
- Personal workflow fit: concise enough for one-person use.
- Technical preservation: `AIVoice`, `Realtime ASR`, `gpt-realtime-whisper`, `batch`, `Cursor`, and `GitHub` remain recognizable.

## Expected output shape

### Slack

明日の朝までに、AIVoiceのRealtime ASRのfallback確認をお願いします。

- `gpt-realtime-whisper` が失敗したときに batch へ落ちるか確認
- Cursorの履歴から辞書追加するとき、追加対象が分かるようにする
- 完了後、GitHubチケットに進捗を記録

Score: 5/5. Concise chat message, no greeting/signature, actions are easy to scan.

### Email

明日の朝までに、AIVoiceのRealtime ASRのfallback確認をお願いします。

`gpt-realtime-whisper` が失敗した場合に batch で処理されるかを確認してください。あわせて、Cursorの履歴から辞書追加する際に、何が追加されるのか分かるようにしてください。

完了後、GitHubのチケットに進捗を記録してください。

Score: 4/5. Polite body text, but still compact. No invented recipient or signature.

### Memo

- 明日の朝までにAIVoiceのRealtime ASR fallbackを確認する
- `gpt-realtime-whisper` 失敗時に batch へ落ちるか見る
- Cursorの履歴から辞書追加するとき、追加対象が分かるようにする
- 終わったらGitHubチケットへ進捗を書く

Score: 5/5. Neutral personal task memo.

### AI Prompt

AIVoiceの改善作業を進めてください。

- 明日の朝までにRealtime ASRのfallbackを確認する
- `gpt-realtime-whisper` が失敗した場合に batch ASRへ切り替わるか確認する
- Cursorの履歴から辞書追加する際、追加対象が事前に分かるUIにする
- 完了後、GitHubチケットへ進捗を記録する

出力には、確認結果と未確認事項を含めてください。

Score: 4/5. Clear assistant instruction. The final output requirement is inferred from “進捗を書いて” but should not become too expansive.

### Technical

- Deadline: 明日の朝まで
- Target: AIVoice Realtime ASR fallback
- Check: `gpt-realtime-whisper` failure falls back to batch ASR
- UI fix: Cursor history dictionary add should show the exact candidate before adding
- Follow-up: write progress to GitHub issues

Score: 5/5. Preserves tokens and organizes implementation facts.

## Live API evaluation: 2026-07-03

Model: `gpt-4o-mini`  
Temperature: `0.1`  
Runs: 3 per preset, using the shared input above.

### Findings before prompt tuning

- Slack sometimes collapsed multiple tasks into prose instead of scan-friendly bullets.
- Some outputs weakened the requested action from `確認する` / `見る` to `検討する`.
- Some outputs localized the technical token `batch` to `バッチ`.
- AI Prompt and Technical Note did not reliably produce distinct structures.

### Changes applied

- Added guardrails to preserve Latin technical tokens such as `batch`, `fallback`, `Realtime ASR`, `gpt-realtime-whisper`, `Cursor`, and `GitHub`.
- Added guardrails to avoid weakening spoken actions such as `確認する`, `見る`, and `分かるようにする`.
- Lowered Polish temperature to `0.1` for more stable rewrites.
- Made multi-task output shape explicit:
  - Slack: lead sentence plus compact bullets.
  - Memo: bullets for multiple tasks.
  - AI Prompt: `目的`, `タスク`, `条件`, `報告` headings.
  - Technical Note: labeled bullets such as `Deadline`, `Check`, `UI`, `Follow-up`.

### Final live evaluation result

| Preset | `batch` preserved | `gpt-realtime-whisper` preserved | No weakening/localization | Preset shape |
| --- | --- | --- | --- | --- |
| Slack | 3/3 | 3/3 | 3/3 | 3/3 |
| Email | 3/3 | 3/3 | 3/3 | 3/3 |
| Memo | 3/3 | 3/3 | 3/3 | 3/3 |
| AI Prompt | 3/3 | 3/3 | 3/3 | 3/3 |
| Technical Note | 3/3 | 3/3 | 3/3 | 3/3 |
