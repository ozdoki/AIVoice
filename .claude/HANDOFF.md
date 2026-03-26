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
| api_key を Credential Manager に保存 | ✅ | keyring クレート経由。#[serde(skip_serializing)] で JSON 除外・invoke では受信可能 |
| モード永続化（再起動後も維持） | ✅ | settings.mode として保存 |
| device_id 無効時のデフォルトデバイスフォールバック | ✅ | GetDevice 失敗時に GetDefaultAudioEndpoint へ |
| set_mode 後のトレイ状態同期 | ✅ | 録音中/処理中/待機中に応じてトレイを更新 |
| 設定画面のエラー表示 | ✅ | デバイス取得・保存失敗を UI に表示、✕で閉じられる |
| クリップボード競合対策 | ✅ | GetClipboardSequenceNumber で外部変更を検知し復元をスキップ |
| session_service 分離（AppHandle 不要の純粋関数） | ✅ | TextInjector trait でモック可能 |
| ユニットテスト（12本） | ✅ | settings / mode / session_service をカバー |
| tauri-plugin-global-shortcut 依存削除 | ✅ | capabilities/default.json からも削除 |
| session://state-changed Rust emit（Phase 0） | ✅ | SessionUiEvent を全ウィンドウへ broadcast。UI はイベント購読のみ |
| audio://level Rust emit（WASAPI RMS） | ✅ | キャプチャループ内で RMS 計算 → UnboundedSender 経由で転送タスクが emit |
| フローティングバー（Phase 1） | ✅ | floating-bar ウィンドウ。ピル型・透明背景・音量連動波形バー・タスクバー上配置 |
| MockTrigger 削除 | ✅ | 開発 UI から完全に除去 |

---

## 触ってはいけない場所・制約

- `inject.rs` の clipboard 経路: `KEYEVENTF_UNICODE` は使わないこと
  （日本語 IME が composition モードで余計な文字を挿入する）
- `hotkey.rs` の `RegisterHotKey`: `tauri-plugin-global-shortcut` や `WH_KEYBOARD_LL` に戻さないこと
  （plugin は F4 漏れ、WH_KEYBOARD_LL は Windows 言語切替 Ctrl+Shift と競合）

---

## 残タスク（優先順）

### 高優先度
1. **Phase 2: 音声入力履歴**
   - 1録音セッション = 1エントリ（id / created_at / mode / raw_text / final_text / inject_status）
   - `history.rs` を追加、`AppState` に `VecDeque<HistoryEntry>` を持たせる
   - 永続化: `tauri-plugin-store` で `history.json` に分離（最大 100 件）
   - UI: メインウィンドウに履歴タブを追加、コピー / 削除のみ（再入力はフォーカス問題があるため後回し）
   - inject 失敗時も履歴に残す（`inject_status=failed`）

### 低優先度
- streaming / ハンズフリー など、MVP 計画書 `docs/aivoice-mvp-plan.md` 参照

---

## アーキテクチャメモ（複数ウィンドウ構成）

- **main ウィンドウ**: 設定・モード切替・セッション表示。ホットキーイベントを受けて `invoke` を呼ぶ唯一の起点
- **floating-bar ウィンドウ**: 受信専用。`session://state-changed` と `audio://level` をリッスンし表示するだけ。`invoke` は停止ボタンのみ
- 状態管理: Rust（AppState）が真のソース。UI は全てイベント購読で同期する

---

## 最終更新

2026-03-26（Phase 0/1 完了: セッション状態 Rust emit・フローティングバー・api_key バグ修正）
