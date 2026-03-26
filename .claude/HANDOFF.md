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
| 設定永続化（api_base_url / api_key / api_model / polish_model / device_id） | ✅ | tauri-plugin-store |
| モード永続化（再起動後も維持） | ✅ | settings.mode として保存 |
| session_service 分離（AppHandle 不要の純粋関数） | ✅ | TextInjector trait でモック可能 |
| ユニットテスト（11本） | ✅ | settings / mode / session_service をカバー |
| tauri-plugin-global-shortcut 依存削除 | ✅ | capabilities/default.json からも削除 |

---

## 触ってはいけない場所・制約

- `inject.rs` の clipboard 経路: `KEYEVENTF_UNICODE` は使わないこと
  （日本語 IME が composition モードで余計な文字を挿入する）
- `hotkey.rs` の `RegisterHotKey`: `tauri-plugin-global-shortcut` や `WH_KEYBOARD_LL` に戻さないこと
  （plugin は F4 漏れ、WH_KEYBOARD_LL は Windows 言語切替 Ctrl+Shift と競合）

---

## 残タスク（優先順）

### 高優先度
1. **api_key 平文保存の改善**
   現状 `tauri-plugin-store` の JSON に平文保存。Windows Credential Manager または DPAPI へ移行。

2. **device_id フォールバック**
   保存済みデバイスが消えた場合（USB デバイス取り外しなど）にデフォルトに戻す処理がない。

### 中優先度
3. **MockTrigger コンポーネントの削除**
   `src/components/MockTrigger.tsx` が残っている。開発用途のみなら削除または dev ガード。

4. **`set_mode` 失敗時の tray 状態ズレ**
   `commands.rs::set_mode` が失敗した場合、tray のモード表示が実際の状態と一致しない可能性がある。
   （App.tsx 側の楽観的更新バグは修正済み）

### 低優先度
5. **streaming / 履歴 / ハンズフリー** など

---

## 最終更新

2026-03-26（ホットキーを WH_KEYBOARD_LL → RegisterHotKey に移行。Ctrl+Shift が Windows 言語切替と競合していた問題を解決）
