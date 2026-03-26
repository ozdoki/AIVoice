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

## 003: Polish モードは失敗時 raw にフォールバックする

**決定**: `polish_text()` が API エラーや空レスポンスを返した場合、エラーを表示せず
そのまま raw テキストを注入する。

**理由**:
音声入力の主目的は「テキストを素早く挿入すること」であり、
Polish API の失敗でテキスト注入が完全に止まるのは UX 上許容できない。
ユーザーは raw テキストが入っても会話の文脈を理解できるため、
サイレントフォールバックが最善と判断した。

**日付**: 2026-03-26 Polish モード実装セッション
