# Polish Preset Evaluation

## Reproducible preset evaluation (Issue #30)

The current evaluation uses the anonymized fixed corpus in
`src-tauri/eval/polish-preset-corpus.json` and the
`polish_preset_eval` example. It exercises the same `polish::polish_text`
request path as KoeType. The baseline contains two representative cases for
each of `slack`, `email`, `memo`, `prompt`, and `technical` (10 API calls).
Custom instructions and deep context are disabled so the preset itself is
being evaluated.

The completed Issue #30 runs and final outputs are recorded in
[Polish Preset Evaluation Results](polish-preset-evaluation-results.md).

### Six-axis rubric

Score every axis as 0, 1, or 2. Enter the scores in the `human` object of each
saved result. A score of 2 means ready for daily use, 1 means usable with a
minor defect, and 0 means a material failure.

| Axis | 0 | 1 | 2 |
| --- | --- | --- | --- |
| Meaning fidelity | Meaning or action is changed | Meaning is mostly retained, with minor drift | All intent, force, conditions, and uncertainty are retained |
| No invention | Adds a material fact, action, status, or identity | Adds harmless but unnecessary wording | Adds no information |
| Cleanup quality | Leaves disruptive filler/repetition or creates an error | Minor awkwardness remains | Cleans speech naturally without flattening it |
| Preset fit | Wrong destination/shape | Recognizable preset with a minor format issue | Immediately usable in the selected destination |
| Readability | Hard to scan or unnaturally fragmented | Understandable with a minor flow issue | Concise, natural, and easy to scan |
| Token and structure preservation | Corrupts a protected token or critical structure | Minor non-critical formatting drift | Preserves protected tokens and required structure |

A case passes only when:

- all machine checks pass;
- no human axis is 0; and
- the human total is at least 10/12.

A preset passes when both of its cases pass and its mean human score is at
least 10/12. The full suite passes only when all five presets pass. Machine
checks are deliberately strict tripwires for missing protected terms,
forbidden claims, mandatory headings, bullets, and paragraph separation; they
do not replace human review.

### Commands

Run these commands from `src-tauri`:

```powershell
# Validate and list the corpus without reading credentials or calling an API
cargo run --example polish_preset_eval -- --list
cargo run --example polish_preset_eval -- --dry-run --output polish-preset-eval-results-dry-run.json

# Baseline: exactly 10 calls with the default gpt-5.4-mini model
cargo run --example polish_preset_eval -- --output polish-preset-eval-results-baseline.json

# Re-test only a failing preset or case after a prompt change
cargo run --example polish_preset_eval -- --preset prompt --output polish-preset-eval-results-prompt-r2.json
cargo run --example polish_preset_eval -- --case prompt-pr-workflow --output polish-preset-eval-results-prompt-pr-r3.json

# Choose another model explicitly
cargo run --example polish_preset_eval -- --model gpt-5.4-mini --output polish-preset-eval-results-baseline.json
```

Live execution reads the KoeType credential named `aivoice` / `api_key` from
Windows Credential Manager into memory. The key is never printed or included
in the report. Result filenames matching `polish-preset-eval-results*.json`
are ignored by Git because they contain generated model output.

### Low-usage improvement loop

1. Run the 10-call baseline once and complete all human scores.
2. Classify failures by shared guardrail versus one preset; change the narrowest
   relevant prompt and add/update an offline assertion before another API call.
3. Re-run only failed cases. Do not re-run passing cases while tuning an
   unrelated preset.
4. When all failed cases pass, run the full 10-case suite once as regression.
5. Re-run only borderline or nondeterministic cases once. Stop when all preset
   gates pass; do not spend calls seeking stylistic perfection beyond the rubric.

ASR recognition accuracy is outside this evaluation. Inputs intentionally use
the intended words, so errors such as `改行` becoming `開業` must be tracked
separately. A single optional custom-instruction priority case may be added
later, but preset-only quality remains the primary gate.

## Historical shared fixture

The material below is retained as historical prompt-review evidence. It is not
the current Issue #30 scorecard.

## Shared input

```text
えっと明日の朝までにKoeTypeのリアルタイムASRのフォールバック確認をして、もしgpt-realtime-whisperが失敗したらbatchで入るか見て、あとCursorの履歴から辞書追加するとき何が追加されるか分かるようにして、終わったらGitHubのチケットに進捗を書いてください。
```

## Rubric

- Meaning preserved: no invented facts, names, deadlines, or completion claims.
- Filler removed: hesitation and repeated starts are removed.
- Preset fit: output shape clearly matches Slack, email, memo, AI prompt, or technical note.
- Personal workflow fit: concise enough for one-person use.
- Technical preservation: `KoeType`, `Realtime ASR`, `gpt-realtime-whisper`, `batch`, `Cursor`, and `GitHub` remain recognizable.

## Expected output shape

### Slack

明日の朝までに、KoeTypeのRealtime ASRのfallback確認をお願いします。

- `gpt-realtime-whisper` が失敗したときに batch へ落ちるか確認
- Cursorの履歴から辞書追加するとき、追加対象が分かるようにする
- 完了後、GitHubチケットに進捗を記録

Score: 5/5. Concise chat message, no greeting/signature, actions are easy to scan.

### Email

明日の朝までに、KoeTypeのRealtime ASRのfallback確認をお願いします。

`gpt-realtime-whisper` が失敗した場合に batch で処理されるかを確認してください。あわせて、Cursorの履歴から辞書追加する際に、何が追加されるのか分かるようにしてください。

完了後、GitHubのチケットに進捗を記録してください。

Score: 4/5. Polite body text, but still compact. No invented recipient or signature.

### Memo

- 明日の朝までにKoeTypeのRealtime ASR fallbackを確認する
- `gpt-realtime-whisper` 失敗時に batch へ落ちるか見る
- Cursorの履歴から辞書追加するとき、追加対象が分かるようにする
- 終わったらGitHubチケットへ進捗を書く

Score: 5/5. Neutral personal task memo.

### AI Prompt

KoeTypeの改善作業を進めてください。

- 明日の朝までにRealtime ASRのfallbackを確認する
- `gpt-realtime-whisper` が失敗した場合に batch ASRへ切り替わるか確認する
- Cursorの履歴から辞書追加する際、追加対象が事前に分かるUIにする
- 完了後、GitHubチケットへ進捗を記録する

出力には、確認結果と未確認事項を含めてください。

Score: 4/5. Clear assistant instruction. The final output requirement is inferred from “進捗を書いて” but should not become too expansive.

### Technical

- Deadline: 明日の朝まで
- Target: KoeType Realtime ASR fallback
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

## Regression fixture: semantic paragraphing (Issue #27)

This fixture covers explanatory speech that changes semantic role several times without becoming a task list.

### Input

```text
この件はスルーでもよいと思いますが、知見として共有します。今このスペースにはiframeでGoogleスライドを載せていて、4枚目へ進むと動画が自動再生されます。この方式なら、各ユーザーが近づいたときに最初から動画を再生する仕組みを作れるかもしれません。まだ十分に検証できていませんが、ひとまず認識してもらえると助かります。
```

### Expected semantic boundaries

1. Preface and reason for sharing.
2. Current iframe / Google Slides / autoplay behavior.
3. Possible per-user mechanism inferred from that behavior.
4. Uncertainty and closing request.

For the Slack preset with custom instructions that prohibit implicit structure, these four units must be prose paragraphs separated by one blank line. Do not add headings or bullets. A short single-topic input must remain one paragraph.

### Pass criteria

- All four semantic roles remain recognizable and appear in four paragraphs.
- Paragraph separators are exactly `\n\n`.
- The output keeps the speaker's uncertainty and conversational temperature.
- No facts, conclusions, greetings, headings, bullets, or meta commentary are added.
- Copy, history reload, reinjection, and Polish rerun preserve the paragraph separators.
