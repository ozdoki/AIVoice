# KoeType

KoeType は、個人利用を前提にした Windows 先行の音声入力アプリです。`Raw` モードでは発話をできるだけそのまま入力し、`Polish` モードでは発話を用途に合う文章へ整えてから入力します。

BYOK（自分の OpenAI API キー）で使い、設定・辞書・履歴はローカルに保存します。

## OSS としての目的

KoeType は、個人が安全に BYOK で使える音声入力アプリの実装例として公開しています。Tauri / Rust / React による Windows デスクトップアプリで、OpenAI 互換 ASR、ローカル設定、Windows Credential Manager、履歴復元、テキスト注入を組み合わせた実用的な構成を検証します。

特に以下を重視します。

- API キーをアプリ内で安全に扱う
- 音声入力、文章整形、履歴復元をローカルファーストに設計する
- Tauri コマンドと Windows ネイティブ処理の境界を小さく保つ
- 依存関係とデスクトップ権限を継続的に見直す

## 現在の実装

- Tauri 2 / React / Rust によるデスクトップアプリ
- グローバルホットキー
  - 押している間録音
  - ハンズフリー録音
  - Raw / Polish モード切替
- Windows 音声入力
  - マイク選択
  - WASAPI 録音
  - 録音レベル表示
- OpenAI 互換 ASR
  - `gpt-realtime-whisper` を基本設定
  - Realtime final が取れない場合の batch ASR フォールバック
- Polish
  - Slack風、メール風、メモ風、AIプロンプト風、技術メモ風のプリセット
  - カスタム指示
  - 前面アプリ情報を使った軽い文体補助
- 個人辞書
  - 固有名詞・技術語の登録
  - 履歴からの辞書候補
- 個人用スニペット
  - 音声キューを定型文、URL、署名などへ展開
- 履歴・復元
  - 履歴検索
  - ピン留め
  - コピー、再注入、辞書追加、Polish再実行
  - 失敗・中断した録音の復元候補
- UI
  - メイン画面
  - フローティングバー
  - 初回セットアップ
  - 設定カテゴリ: `入力`、`AI`、`辞書`、`履歴`、`詳細`

## 対象外

このリポジトリでは、当面以下は扱いません。

- 選択テキスト編集
- チーム機能
- 共有辞書
- SSO
- 管理画面
- 課金設計

## セットアップ

```powershell
npm install
```

OpenAI API キーはアプリの設定画面から保存します。キーは Windows Credential Manager に保存され、設定 JSON には書き出しません。

## セキュリティとプライバシー

- API キーは Windows Credential Manager に保存し、設定 JSON には保存しません。
- `.env` / `.env.*` は Git 管理対象外です。
- 履歴、辞書、スニペット、利用量、復元データはローカル保存です。
- 前面アプリ情報を使う設定では、アプリ名とウィンドウタイトルのみをプロンプト補助に使います。入力欄本文は読み取りません。
- 脆弱性や秘密情報に関わる報告は [SECURITY.md](SECURITY.md) を参照してください。

## 開発コマンド

```powershell
npm run dev
npm run build
```

Tauri release exe をビルドして起動中アプリへ反映する場合:

```powershell
npm run app:restart
```

このプロジェクトでは、実行中アプリへの反映確認は `npm run app:restart` を使います。release build は `index.html` を同梱して起動するため、localhost の dev server に依存しません。

## 検証コマンド

```powershell
cmd /c .\node_modules\.bin\tsc.cmd --noEmit
cmd /c .\node_modules\.bin\vite.cmd build
cd src-tauri
cargo fmt --check
cargo check
cargo test --no-run
```

`cargo test --no-run` はテストバイナリのビルド確認です。環境によってはテスト実行フェーズで追加調査が必要です。

## ドキュメント

- 企画・設計の一次資料: [docs/aivoice-mvp-plan.md](docs/aivoice-mvp-plan.md)

## ロードマップ

- [#10 Realtime ASR と入力中フィードバック](https://github.com/ozdoki/AIVoice/issues/10)
- [#11 個人辞書UXの改善](https://github.com/ozdoki/AIVoice/issues/11)
- [#12 Polish プリセットと出力品質改善](https://github.com/ozdoki/AIVoice/issues/12)
- [#13 アプリ別スタイル補助](https://github.com/ozdoki/AIVoice/issues/13)
- [#14 個人用スニペット機能](https://github.com/ozdoki/AIVoice/issues/14)
- [#15 初回セットアップUX](https://github.com/ozdoki/AIVoice/issues/15)
- [#16 メイン画面とフローティングバーのブラッシュアップ](https://github.com/ozdoki/AIVoice/issues/16)
- [#17 設定画面の情報設計整理](https://github.com/ozdoki/AIVoice/issues/17)
- [#18 履歴・復元UXの強化](https://github.com/ozdoki/AIVoice/issues/18)
- [#19 README と現状ドキュメント更新](https://github.com/ozdoki/AIVoice/issues/19)
- [#20 録音中ライブ文字表示をFloatingBarに追加する](https://github.com/ozdoki/AIVoice/issues/20)

## 方針

- 個人利用を優先する
- Windows 先行で作る
- API キーはローカルで安全に扱う
- 履歴、辞書、スニペット、設定はローカル保存を基本にする
- 音声入力の安定性と復旧性を優先する

## コントリビューション

開発方針、対象外スコープ、Pull Request の確認項目は [CONTRIBUTING.md](CONTRIBUTING.md) を参照してください。

## ライセンス

MIT License です。詳細は [LICENSE](LICENSE) を参照してください。
