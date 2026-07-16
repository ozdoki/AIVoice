# 選択テキスト取得 Windows 実機QA

## セキュリティ前提

- 選択本文、パスワード、clipboard本文をログ・スクリーンショットへ残さない。
- 操作開始時の前景HWND/PIDと、取得完了時の前景HWND/PID・UIA focused elementが一致しない場合は失敗すること。
- 同一focused element内でselectionだけが瞬間的に変わる競合はUIAで完全に識別できず、残余リスクとして扱う。
- UIA provider呼び出しは安全に強制キャンセルできない。2秒で呼び出し元へtimeoutを返した後もblocking workerが終了するまでsingle-flight permitを保持し、次の取得は拒否する。
- partial SendInput、clipboard更新待ちtimeout、focus mismatchでは更新主体を識別できないため、元clipboardを復元せず外部更新を優先する。

## UI Automation優先経路

1. メモ帳で単一範囲を選択し、選択本文を取得できることを確認する。
2. EdgeまたはChromeの通常ページ、VS Code、Word・Excel・Outlookで単一範囲を選択し、UIA対応状況と取得方式を確認する。
3. 複数選択、空選択、20,000文字超がそれぞれ異なるエラーになることを確認する。
4. 取得操作中に別windowまたは同一window内の別controlへfocusを移し、本文を返さず失敗することを確認する。

### 実施記録

- 2026-07-15、Chromeのローカルdata URL（ページ名 `KoeType Browser QA`）で、textareaの `日本語 Alice 42 😀` とcontenteditableの `日本語 Bob 73 😃` をそれぞれ明示選択した。
- 各選択で `Ctrl + Shift + F9` により選択音声編集の録音を開始し、約1秒後にEscapeでキャンセルした。いずれも選択原文は変化せず、KoeTypeがIdleへ戻り、エラー表示がないことを確認した。
- Chrome専用runtimeは互換性エラーで利用できなかったため、WindowsのアプリレベルQAとして実施した。既存の業務タブは操作していない。
- ChatGPT desktopは安全規約により操作していない。
- 実機QAでF9開始を一度見落とした原因は、選択音声編集の録音中も通常SessionPanelの「待機中」が中央に残り、専用案内が下部に表示されていたことだった。録音中は通常SessionPanelを中央の専用録音表示へ置き換えるUX修正を行った。
- 同日の最新production EXEでChromeのローカルQAページを再確認した。contenteditableのテキストを明示選択してF9を1回押すと、中央に「選択テキストの編集指示を録音中」「F9で編集案生成 / Escapeでキャンセル」が表示され、元テキストが変化しないことを目視確認した。
- ChatGPTデスクトップ版は操作制約により自動QA対象外とした。

## 保護入力・権限差

1. ブラウザ、Windows設定、パスワード管理アプリのpassword欄で取得が拒否されることを確認する。
2. focused elementまたはancestorの`IsPassword`取得を妨げる環境でfail-closedになることを確認する。
3. 管理者権限で起動したメモ帳などを通常権限のKoeTypeから操作し、UIPIエラーになり原文・clipboardを不用意に変更しないことを確認する。

## Ctrl+C fallbackとclipboard保全

1. UIA TextPattern非対応のTerminalなどでCtrl+C fallbackを確認する。
2. 修飾キーを押したまま操作し、bounded wait後にコピーを送らず失敗することを確認する。
3. Unicode text、HTML、RTF、画像、ファイル一覧、アプリ固有formatを同時にclipboardへ置き、取得後に全formatが復元されることを確認する。
4. 複製不能なclipboard formatを含む場合、Ctrl+C送信前に中止することを確認する。
5. コピー待機中または復元直前に別アプリからclipboardを更新し、その外部更新を上書きせず、取得成功時はwarningが返ることを確認する。
6. clipboardを別プロセスでlockし、本文をエラーへ含めず安全に失敗することを確認する。
7. Office、ブラウザ、VS Code、Terminalで取得前後のclipboard sequenceと全formatを比較する。
