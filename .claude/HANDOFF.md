# AIVoice 引き継ぎ文

> このファイルは毎セッション末に更新する。
> 新しいセッション開始時の指示: 「このファイルと .claude/DECISIONS.md を読んで、現状理解を5行で要約してから作業を始めて」

## 目的

Windows 向け音声入力アプリ（Tauri 2 + Rust + React）。
F4 を押している間マイクで録音し、離すと音声認識してカーソル位置にテキストを注入する。
Raw モード（そのまま）と Polish モード（LLM で整形）を F5 で切替。

---

## 今どこまで終わったか

| 機能 | 状態 | 備考 |
|------|------|------|
| WH_KEYBOARD_LL ホットキー（F4/F5） | ✅ | LRESULT(1) でキー消費 |
| WASAPI マイク録音 | ✅ | |
| OpenAI 互換 ASR（Whisper） | ✅ | api_key 空時は MockSpeechProvider |
| テキスト注入（clipboard + Ctrl+V） | ✅ | IME 干渉回避のため clipboard 優先 |
| Polish モード（/chat/completions） | ✅ | 失敗時 raw フォールバック |
| システムトレイ常駐 | ✅ | X ボタン → hide |
| 設定永続化（api_base_url / api_key / api_model / polish_model） | ✅ | tauri-plugin-store |
| モード永続化（再起動後も維持） | ✅ | settings.mode として保存 |

---

## 触ってはいけない場所・制約

- `inject.rs` の clipboard 経路: `KEYEVENTF_UNICODE` に戻さないこと
  （日本語 IME が composition モードで余計な文字を挿入する）
- `hotkey.rs` の `WH_KEYBOARD_LL`: `tauri-plugin-global-shortcut` に戻さないこと
  （F4 が他アプリに漏れる）

---

## Codex レビューで判明した穴（2026-03-26）

実装済み機能の「信頼性と失敗時 UX」に以下の問題がある。

1. **Mock が暗黙的すぎる**
   api_key 空のとき MockSpeechProvider に無言でフォールバックする。
   ユーザーには「動いているように見えるが実際は未設定」という悪い失敗になる。

2. **ホットキーが生キー（F4/F5）固定**
   計画書では「修飾キー付き前提」。現状は誤爆コストが高い。
   他アプリへ渡さず LRESULT(1) で完全消費しているため競合リスクが高い。

3. **クリップボードを注入後に復元していない**
   毎回クリップボードが上書きされたままになる。常用時の副作用として問題。

4. **エラー時に backend 状態が Processing のまま戻らないケースがある**
   `commands.rs` の途中失敗で `RecordingState::Idle` まで戻らない可能性がある。
   トレイ常駐アプリとして失敗が見えなさすぎる。

5. **ホットキー処理が frontend（App.tsx）に依存している**
   emit → React の listen という経路のため、WebView 初期化前後の異常に弱い。

6. **マイク選択・ホットキー設定がない**
   MVP 計画書（L83/L84）に明記されているが未実装。

---

## 次にやること（優先順）

### フェーズ1: 信頼性の是正（最優先）

1. **Mock の明示化**
   api_key 未設定時に UI / トレイで警告を出す。MockSpeechProvider は明示的な開発フラグでのみ有効にする。

2. **エラー時の状態復旧**
   `stop_recording_session` の途中失敗で必ず `RecordingState::Idle` に戻るよう finally 相当の処理を追加。
   エラー内容をトレイ通知または UI に表示する。

3. **クリップボードの退避・復元**
   `inject.rs` で貼り付け前にクリップボードを退避し、注入後に復元する。

### フェーズ2: 実用性の底上げ

4. **ホットキーの安全化**
   修飾キー付き既定値（例: `Ctrl+Shift+Space`）に変更。
   設定画面でキー変更可能にする。

5. **フローティング状態表示**
   録音中 / 処理中 / 現在モード / 直近エラーを常に見えるようにする。

6. **マイク選択**
   設定画面でマイクデバイスを選択できるようにする。

### フェーズ3: 体験改善・品質担保

7. **テストの追加**
   mode::route、設定保存、Polish フォールバック、注入フォールバック、失敗時状態復旧のテスト。
   現状テストコードはゼロ。

8. **ハンズフリー / streaming / 履歴** など

---

## 最終更新

2026-03-26（Codex レビューに基づき全面更新）
