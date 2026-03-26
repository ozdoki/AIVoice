# AIVoice 引き継ぎ文

> このファイルは毎セッション末に更新する。
> 新しいセッション開始時の指示: 「このファイルと .claude/DECISIONS.md を読んで、現状理解を5行で要約してから作業を始めて」

## 目的

Windows 向け音声入力アプリ（Tauri 2 + Rust + React）。
Ctrl+Shift+F4 を押している間マイクで録音し、離すと音声認識してカーソル位置にテキストを注入する。
Raw モード（そのまま）と Polish モード（LLM で整形）を Ctrl+Shift+F5 で切替。

---

## 今どこまで終わったか

| 機能 | 状態 | 備考 |
|------|------|------|
| RegisterHotKey ホットキー（Ctrl+Shift+F4/F5） | ✅ | WH_KEYBOARD_LL から移行済み |
| WASAPI マイク録音（デバイス選択可） | ✅ | list_capture_devices 実装済み |
| OpenAI 互換 ASR（Whisper） | ✅ | api_key 空時は Err を返す |
| テキスト注入（clipboard + Ctrl+V のみ） | ✅ | KEYEVENTF_UNICODE フォールバック削除済み |
| クリップボード退避・復元 | ✅ | 注入前退避、150ms 待機後復元 |
| Polish モード（/chat/completions） | ✅ | 失敗時 raw フォールバック |
| システムトレイ常駐 | ✅ | X ボタン → hide、ツールチップに状態表示 |
| 設定永続化（api_base_url / api_model / polish_model / device_id） | ✅ | tauri-plugin-store（api_key は除く） |
| api_key を Credential Manager に保存 | ✅ | keyring クレート経由。JSON には書かない |
| モード永続化（再起動後も維持） | ✅ | settings.mode として保存 |
| device_id 無効時のデフォルトデバイスフォールバック | ✅ | GetDevice 失敗時に GetDefaultAudioEndpoint へ |
| set_mode 後のトレイ状態同期 | ✅ | 録音中/処理中/待機中に応じてトレイを更新 |
| MockTrigger を dev ビルド限定に | ✅ | import.meta.env.DEV ガード |
| session_service 分離（AppHandle 不要の純粋関数） | ✅ | TextInjector trait でモック可能 |
| ユニットテスト（12本） | ✅ | settings / mode / session_service をカバー |
| tauri-plugin-global-shortcut 依存削除 | ✅ | capabilities/default.json からも削除 |

---

## 触ってはいけない場所・制約

- `inject.rs` の clipboard 経路: `KEYEVENTF_UNICODE` は使わないこと
  （日本語 IME が composition モードで余計な文字を挿入する）
- `hotkey.rs` の `RegisterHotKey`: `tauri-plugin-global-shortcut` や `WH_KEYBOARD_LL` に戻さないこと
  （plugin は F4 漏れ、WH_KEYBOARD_LL は Windows 言語切替 Ctrl+Shift と競合）

---

## 残タスク（優先順）

### 中優先度
1. **設定画面のエラー表示**
   デバイス列挙失敗・設定保存失敗が UI に表示されない（`SettingsPanel.tsx:34` で握りつぶし）。

2. **クリップボード競合リスク**
   注入時の 150ms クリップボード保持中に別プロセスが書き込むと内容が失われる。
   解消するには OS の遅延貼り付けや排他ロックが必要。

### 低優先度
3. **streaming / 履歴 / ハンズフリー / ホットキー設定 UI** など
   MVP 計画書 `docs/aivoice-mvp-plan.md` 参照。

---

## 最終更新

2026-03-26（セキュリティ・品質改善: api_key を Credential Manager へ移行、device_id フォールバック追加、MockTrigger dev 限定化、set_mode 後トレイ同期）
