# AIVoice 引き継ぎ文

> このファイルは毎セッション末に更新する。
> 新しいセッション開始時の指示: 「このファイルと関連ファイルを読んで、現状理解を5行で要約してから作業を始めて」

## 目的

Windows 向け音声入力アプリ（Tauri 2 + Rust + React）。
F4 を押している間マイクで録音し、離すと音声認識してカーソル位置にテキストを注入する。
Raw モード（そのまま）と Polish モード（LLM で整形）を F5 で切替。

## 今どこまで終わったか

- WH_KEYBOARD_LL ホットキー（F4 録音、F5 モード切替）✅
- WASAPI マイク録音 ✅
- OpenAI 互換 ASR（Whisper）✅
- テキスト注入（clipboard + Ctrl+V）✅
- Polish モード（/chat/completions、失敗時 raw フォールバック）✅
- システムトレイ常駐（X ボタン → hide）✅
- 設定永続化（api_base_url / api_key / api_model / polish_model）✅

## 直近の変更ファイル

- `src-tauri/src/polish.rs`（新規: LLM ポストプロセス）
- `src-tauri/src/mode.rs`（async 化、polish_text 呼び出し）
- `src-tauri/src/settings.rs`（polish_model フィールド追加）
- `src-tauri/src/commands.rs`（mode::route を await）
- `src/components/SettingsPanel.tsx`（Polish Model 入力欄追加）

## 触ってはいけない場所・制約

- `inject.rs` の clipboard 経路: `KEYEVENTF_UNICODE` に戻さないこと
  （日本語 IME が composition モードで処理して余計な文字が挿入される）
- `hotkey.rs` の `WH_KEYBOARD_LL`: `tauri-plugin-global-shortcut` に戻さないこと
  （F4 が他アプリに漏れる）

## 未解決事項

- モードが永続化されていない（再起動すると常に Raw にリセットされる）

## 次にやること

**フェーズB 残作業: モード永続化**

1. `src-tauri/src/settings.rs`: `AppSettings` に `mode: Mode` フィールドを追加
   - `#[serde(default)]` で既存 settings.json と後方互換
2. `src-tauri/src/commands.rs`: `set_mode` コマンドでモード変更時に `settings::save()` も呼ぶ
3. `src-tauri/src/main.rs`: startup の `settings::load()` 後に mode も AppState に反映する

## 最終更新

2026-03-26
