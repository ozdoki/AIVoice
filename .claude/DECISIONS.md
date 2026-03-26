# 設計判断ログ

設計・技術選定の「なぜそうしたか」を記録する。
実装の変更を検討するときは、まずここを確認する。

---

## 001: テキスト注入を clipboard + Ctrl+V にした

**決定**: `inject.rs` のテキスト注入は `KEYEVENTF_UNICODE` ではなく clipboard + Ctrl+V を第一経路にする。

**理由**:
`KEYEVENTF_UNICODE` で日本語テキストを送ると、日本語 IME が composition モードで受け取り
「聞試」「試聞」などの意図しない文字列を混入させるバグが発生した。
クリップボード経由であれば IME を介さずテキストを貼り付けられるため、この問題が回避できる。

**トレードオフ**:
- クリップボードの内容を一時的に上書きする（録音後に短時間クリップボードが汚れる）
- clipboard API が使えない環境では SendInput フォールバックが必要

**日付**: 2026-03 初期実装セッション

---

## 002: グローバルホットキーを RegisterHotKey にした

**決定**: Ctrl+Shift+F4/F5 のグローバルホットキーは Win32 の `RegisterHotKey` API で実装する。

**経緯**:
1. `tauri-plugin-global-shortcut` → F4 が他アプリに漏れる問題で却下
2. `WH_KEYBOARD_LL` + AtomicBool 修飾キー追跡 → Windows 11 の入力言語切替ホットキー（Ctrl+Shift）が OS レベルで横取りし、Ctrl/Shift/F4 イベントがフックコールバックに届かない問題で断念
3. `RegisterHotKey` → OS が修飾キー検出を内部処理するため言語切替との競合なし。keyup は `GetAsyncKeyState` ポーリングで検出

**トレードオフ**:
- Win32 固有の実装になり、macOS/Linux への移植コストが上がる
- RegisterHotKey は keydown のみ通知 → F4 リリースは GetAsyncKeyState ポーリング（10ms 間隔）で検出
- 他アプリが同じホットキーを RegisterHotKey 済みの場合、登録が失敗する

**日付**: 2026-03-26 ホットキー移行セッション

---

## 005: クリップボード復元は外部変更がない場合のみ行う

**決定**: `clipboard_paste()` の 150ms 待機後、`GetClipboardSequenceNumber` でシーケンス番号を比較し、
自分の `set_text` 以外の変更があった場合はクリップボードを復元しない。

**理由**:
待機中に別プロセスがクリップボードを書き換えていた場合、古いテキストで上書きするとユーザーのデータが失われる。
シーケンス番号が `seq_before + 1`（自分の書き込みのみ）であれば復元は安全。それ以上なら外部変更とみなしスキップ。

**トレードオフ**:
- 競合が発生した場合、クリップボードには注入したテキストが残る（削除されない）
- `Win32_System_DataExchange` の `GetClipboardSequenceNumber` を使うため Windows 専用

**日付**: 2026-03-26

---

## 004: api_key を Windows Credential Manager に保存する

**決定**: `AppSettings.api_key` は `tauri-plugin-store` の JSON には書かず、`keyring` クレート経由で Windows Credential Manager に保存する。

**理由**:
`settings.json` は Tauri のアプリデータディレクトリに平文で置かれ、ファイルシステムへのアクセス権があれば誰でも読める。
API キーのような秘密情報は OS が管理する認証情報ストアに分離すべき。

**実装**:
- `AppSettings` の `api_key` に `#[serde(skip_serializing, default)]` を付与
  - `skip_serializing`: JSON 出力時は除外（settings.json には書かない）
  - `default`: JSON 入力時はフィールドがなければ空文字で初期化
  - ※ 当初 `#[serde(skip)]`（シリアライズ・デシリアライズ両方スキップ）だったが、
    フロントエンドからの invoke ペイロードもデシリアライズされず api_key が常に空になるバグがあった。
    `skip_serializing` に変更することで「JSON 保存には書かないが invoke では受け取れる」設計になった。
- `settings::load()` / `save()` が `keyring::Entry` を通じて Credential Manager を読み書き
- サービス名 `"aivoice"` / ユーザー名 `"api_key"` でエントリを識別

**トレードオフ**:
- 既存ユーザーは API キーを再入力する必要がある（旧 JSON からの自動移行なし）
- `keyring` クレートが OS のシークレットバックエンドに依存する（Windows: DPAPI、macOS: Keychain、Linux: Secret Service）

**日付**: 2026-03-26

---

## 006: セッション状態を Rust から全ウィンドウに emit する（Phase 0）

**決定**: 録音開始・処理中・完了・エラーの状態変化は Rust の `commands.rs` から `session://state-changed` イベントとして全ウィンドウへ broadcast する。UI は状態をローカル管理しない。

**理由**:
フローティングバーを追加して複数ウィンドウ構成になると、「ホットキー → App.tsx が invoke → App.tsx がローカル state 更新」という設計では floating-bar が状態を知る手段がなくなる。
Rust を真の状態源にして全ウィンドウへ broadcast することで、ウィンドウ数に依存しない一貫した状態同期ができる。

**実装**:
- `SessionUiEvent { state, mode, final_text, error }` を `commands.rs` で定義
- `start_recording_session` / `stop_recording_session` コマンドが emit
- `App.tsx` は `hotkey://start` / `hotkey://stop` リスナーで `invoke` を呼ぶだけ。`setRecordingState` は `session://state-changed` リスナーに一本化
- `FloatingBar` は `session://state-changed` のみ購読。`invoke` は停止ボタンのみ

**トレードオフ**:
- `hotkey://toggle-mode` だけはまだ App.tsx 側で invoke + setMode しており、mode 変更イベントは session 経由では来ない。floating-bar は recording 開始時の event payload に含まれる mode で表示を更新するため、録音開始前の mode 変更は bar には反映されない（バー表示中は常に最新 mode が届くので実用上問題なし）

**日付**: 2026-03-26

---

## 007: フローティングバーを別ウィンドウで実装する

**決定**: 音声入力中インジケーターは、メインウィンドウ内の UI ではなく Tauri の別ウィンドウ（`floating-bar`）として実装する。

**理由**:
- メインウィンドウは通常 hidden 状態で使われる。別ウィンドウにすることでメインが非表示でもバーを表示できる
- `decorations: false` / `alwaysOnTop: true` / `transparent: true` の組み合わせで他アプリに重なる常時前面ウィンドウを実現できる
- `skipTaskbar: true` でタスクバーを汚さない

**実装**:
- `tauri.conf.json` に `floating-bar` ウィンドウを追加（`visible: false` で起動）
- `src/main.tsx` で `getCurrentWindow().label` を見て `<FloatingBar>` か `<App>` かをルーティング
- `recording` 時に `win.show()` でタスクバー上に表示、`idle` 時に `win.hide()`
- ウィンドウサイズは 300×60px（ピル 280×44px + shadow 余白）

**トレードオフ**:
- `win.show()` 時にフォーカスを奪う可能性がある（未対処。問題が出れば Rust 側から `ShowWindow(SW_SHOWNOACTIVATE)` で対処する）
- Tauri の `core:window:allow-show` など window 系パーミッションを明示的に追加する必要がある

**日付**: 2026-03-26

---

## 008: WASAPI ループ内で RMS を計算し audio://level で emit する

**決定**: マイク音量の可視化は、`wasapi.rs` のキャプチャループ内でバッファごとに RMS を計算し、`tokio::sync::mpsc::UnboundedSender<f32>` 経由で `commands.rs` の転送タスクへ渡し、`audio://level` イベントとして全ウィンドウへ emit する。

**理由**:
- WASAPI バッファは既に `f32` に正規化されており、追加変換なしで RMS を計算できる
- `UnboundedSender` は `Send` かつ同期的に `send` できるため、blocking スレッド（WASAPI ループ）から tokio ランタイムへのブリッジとして自然に使える
- `WasapiInput` が drop されると sender も drop され、受信側タスクが自動終了する（明示的なキャンセル不要）

**実装**:
- `audio::new_input` に `level_tx: Option<UnboundedSender<f32>>` を追加
- `WasapiInput` がそれを保持し `capture_inner` に渡す
- `session_service::start_session_inner` も `level_tx` を受け取り `new_input` へ転送
- `commands::start_recording_session` でチャネルを生成し、転送タスクを spawn
- フロントエンドは `requestAnimationFrame` ループで指数移動平均（α=0.4）を使い 60fps でスムーズに表示

**トレードオフ**:
- バッファイベントは約 10ms 間隔で届くため ~100 イベント/秒が IPC を流れる。現状許容しているが、パフォーマンス問題が出れば Rust 側でスロットリング（50ms ごとに最大値 emit）を検討する

**日付**: 2026-03-26

---

## 003: Polish モードは失敗時 raw にフォールバックする

**決定**: `polish_text()` が API エラーや空レスポンスを返した場合、エラーを表示せず
そのまま raw テキストを注入する。

**理由**:
音声入力の主目的は「テキストを素早く挿入すること」であり、
Polish API の失敗でテキスト注入が完全に止まるのは UX 上許容できない。
ユーザーは raw テキストが入っても会話の文脈を理解できるため、
サイレントフォールバックが最善と判断した。

**日付**: 2026-03-26 Polish モード実装セッション
