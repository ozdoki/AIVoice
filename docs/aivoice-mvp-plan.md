# AIVoice MVP 計画書

## 概要

AIVoice は、`Raw` モードと `Polish` モードを切り替えられる Windows 先行の音声入力アプリとして構想する。  
`Raw` モードでは話した内容をできるだけそのまま入力し、`Polish` モードでは自然な発話を読みやすい文章へ整形して入力する。  
共通の会話キーで音声入力を開始し、別キーでモードを永続的に切り替えることで、Aqua Voice の即時入力感と TypeLess の文章整形体験を一つの操作系に統合する。

この文書は、MVP に必要な設計判断を固定するための一次計画書であり、実装タスクの細分化までは含めない。

## 既存調査サマリ

### Aqua Voice の特徴

- 全アプリで使える音声入力を前面に出している。
- 低遅延のリアルタイム入力と、発話しながらの文章整形を重視している。
- 画面上の文脈を考慮して変換精度を上げる方向性を打ち出している。
- 言語自動切替やスプレッドシート入力など、汎用入力ユーティリティとしての広さが強い。

### TypeLess の特徴

- 自然発話から読みやすい文章へ変換する AI 的な整形体験を前面に出している。
- フィラー除去、言い直し吸収、アプリごとの文体調整に強みがある。
- 選択テキストに対する書き換え命令や要約など、編集アシスタント的な使い方を提供している。
- 長押しホットキーを中心に、思考を止めずに書く体験を重視している。

### 音声入力アプリの一般的な構成

音声入力アプリは概ね、次の要素の組み合わせで成立する。

- グローバルホットキー
- 低遅延マイク取得
- ストリーミング音声認識
- モード別テキスト変換
- フォーカス先アプリへの文字注入
- 設定、履歴、権限、障害時フォールバック

本計画では、Aqua Voice 的な「どこでも使える入力基盤」を土台にし、その上に TypeLess 的な「整形入力」を載せる方針を取る。

## MVP方針

- 対象 OS は `Windows` 先行とする。
- 実装基盤は `Tauri 2 + Rust + Web UI` を前提にする。
- 音声認識は `クラウド優先` とし、OpenAI 互換 API を差し替え可能な構成にする。
- 初期の主用途は `一般テキスト入力` とし、Slack、メール、ドキュメント、フォーム入力などを重視する。
- 操作系は `長押し中心 + ハンズフリー補助` とする。
- 会話キーは共通にし、`Raw` と `Polish` の切替は別キーで行う。
- ホットキーは初版では `修飾キー付き` を前提にして競合と誤作動を抑える。
- 他アプリへの文字入力は `直接入力` を優先し、失敗時は `貼り付け` をフォールバックとして使う。
- 初版では `アカウントなし`、`課金なし`、`ローカル設定のみ` とする。

### モード定義

#### Raw モード

- 発話内容をできるだけそのまま入力する。
- 最低限の ASR 後処理のみ許可する。
- 口述、ラフメモ、短文チャット向けの入力を担う。

#### Polish モード

- 発話内容を自然な文章へ整形して入力する。
- フィラー除去、言い直し吸収、句読点補正、簡易的な箇条書き整形を行う。
- メール、報告文、説明文などの一般文書入力を担う。

## 実装構成

### 全体アーキテクチャ

MVP の処理系は次の流れで構成する。

1. グローバルホットキーで録音開始
2. `WASAPI / IAudioClient3` で低遅延にマイク音声を取得
3. `SpeechProvider` を通して OpenAI 互換のストリーミング音声認識へ送信
4. `ModeRouter` で `Raw` / `Polish` の処理経路を分岐
5. `TextInjector` で現在フォーカス中のアプリへ文字注入
6. 状態表示、履歴、設定保存を UI とローカルストレージで管理

### デスクトップ構成

- 常駐トレイアプリとして動作する。
- 最小の設定ウィンドウを持つ。
- 録音中、待機中、モード状態を示す軽量なフローティング表示を持つ。
- マイク選択、ホットキー設定、プロバイダ設定、履歴設定を変更できる。

### 音声処理構成

- 音声取得は Windows の低遅延 API を使い、会話開始から注入までの遅延を抑える。
- 認識処理は OpenAI 互換 API に抽象化し、将来のベンダー差し替えを容易にする。
- `Raw` は生テキスト寄り、`Polish` は整形寄りの後段処理を適用する。
- 初版では画面文脈理解や選択テキスト編集は載せない。

### 文字注入構成

- 第一経路は `SendInput` 等による直接入力とする。
- 失敗時の第二経路としてクリップボード経由の貼り付けを使う。
- アプリごとに相性が悪い場合は、将来 `強制貼り付け` のような設定で逃がせる構造にする。
- `UIPI` 制約により昇格アプリへの注入は失敗する可能性があるため、初版の正式サポートは通常権限のアプリに限定する。

## 主要インターフェース

### SpeechProvider

責務:

- 音声認識 API への接続を抽象化する。
- ストリーミング開始、部分結果受信、停止、失敗通知を扱う。
- 差し替え可能な OpenAI 互換プロバイダ境界を提供する。

想定設定項目:

- `baseUrl`
- `apiKey`
- `asrModel`
- `languageMode` (`auto` / `manual`)
- `endpointKind` (`streaming` / `batch`)

### PolishProcessor

責務:

- 認識後の生テキストを読みやすい文章へ整形する。
- フィラー除去、言い直し吸収、句読点補正、箇条書き整形を担当する。
- モードや将来の文体ルールに応じて整形方針を切り替えられる形にする。

### TextInjector

責務:

- フォーカス中の入力先へ文字列を注入する。
- 直接入力と貼り付けの二経路を管理する。
- 失敗時のフォールバックや、対象アプリ別の戦略切替を扱う。

### AppSettings

責務:

- アプリの永続設定を保持する。
- 主要な保持対象は `mode` `hotkeys` `provider` `history` `injectionOverrides` とする。
- 初版ではローカル保存のみとし、同期機能は持たない。

## テスト計画

### 単体テスト

- `ModeRouter` が正しく `Raw` / `Polish` に分岐すること
- `PolishProcessor` の整形ルールが期待通りに動作すること
- `TextInjector` の直接入力失敗時に貼り付けへフォールバックすること
- 設定の保存と復元が正しく行われること

### 結合テスト

- 長押し開始から partial 表示、release 後の final 注入までが一連で動くこと
- モード切替が UI と永続設定へ反映されること
- ハンズフリー開始と終了が安定して切り替わること
- API 失敗時に UI と内部状態が壊れず復帰できること

### 手動 E2E

- メモ帳で短文と長文を入力できること
- ブラウザの textarea で入力できること
- Slack や Discord 相当の入力欄で操作感に問題がないこと
- スプレッドシート系入力欄で最低限の注入が成立すること
- 日英混在、箇条書き、言い直し、短いポーズを含む発話で破綻しないこと

### 失敗系シナリオ

- マイク未接続
- API キー不正
- ネットワーク切断
- 昇格アプリへの注入失敗
- ホットキー競合

## 前提・除外事項

### 前提

- この文書は MVP の設計方針を固定するための一次計画書である。
- 実装フェーズでは、この文書を起点としてプロジェクト雛形作成と技術検証タスクの分解を行う。
- macOS 展開を見据え、Windows 固有処理は音声取得、ホットキー、文字注入の境界に閉じ込める。

### 初版スコープ外

- 選択テキスト編集
- 要約、検索、外部アクションなどの音声エージェント機能
- macOS 対応
- ローカル完結のオフライン ASR
- アカウント、同期、課金
- 高度な画面文脈理解

## 調査ソース

- [Aqua Voice Home](https://aquavoice.com/)
- [Aqua Voice FAQ](https://aquavoice.com/faq)
- [Aqua Voice Avalon API](https://aquavoice.com/avalon-api)
- [Typeless Home](https://www.typeless.com/)
- [Typeless Pricing](https://www.typeless.com/pricing)
- [Typeless Voice Superpowers](https://www.typeless.com/help/release-notes/macos/voice-superpowers)
- [Typeless Windows Beta](https://www.typeless.com/help/release-notes/windows/introducing-typeless-windows-app-beta)
- [Microsoft Learn: IAudioClient3](https://learn.microsoft.com/ja-jp/windows/win32/api/audioclient/nn-audioclient-iaudioclient3)
- [Microsoft Learn: SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [Microsoft Learn: RegisterHotKey](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey)
- [Tauri Plugin Docs](https://v2.tauri.app/plugin/)
