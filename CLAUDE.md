# AIVoice

## プロジェクト概要
- Windows 先行の音声入力アプリ。
- `Raw` モードは発話をできるだけそのまま入力する。
- `Polish` モードは自然な文章へ整形して入力する。
- MVP の実装基盤は `Tauri 2 + Rust + Web UI` を前提にする。

## 現在の主要資料
- 一次計画書: `docs/aivoice-mvp-plan.md`
- リポジトリ概要: `README.md`

## セッション開始時の手順
- まず `.claude/HANDOFF.md` を読み、現状を把握してから作業を始める。
- 設計判断の背景は `.claude/DECISIONS.md` を参照する。
- 長く維持したいルールはここに書かず、`.claude/rules/` の対象ファイル別ルールを参照する。

## ビルド方法
- `cargo check`: `powershell.exe -Command "Set-Location 'E:\github\AIVoice\src-tauri'; C:\Users\naniw\.cargo\bin\cargo.exe check"`
- `pnpm tauri dev`: フロントエンド + Rust を同時に起動する開発モード

## 実装済み機能（2026-03-26）
- `hotkey.rs`: WH_KEYBOARD_LL フック。F4 = 録音開始/停止、F5 = Raw/Polish 切替
- `inject.rs`: clipboard + Ctrl+V 第一経路でテキスト注入（IME 干渉回避）
- `tray.rs`: システムトレイ常駐。X ボタン → hide（終了しない）
- `polish.rs`: `/chat/completions` 呼び出しによる LLM 整形。失敗時は raw にフォールバック
- `speech/openai_compatible.rs`: OpenAI 互換 ASR（Whisper）
- `audio/`: WASAPI マイク録音

## 絶対に変えてはいけない制約
- テキスト注入は `KEYEVENTF_UNICODE` ではなく clipboard+Ctrl+V を使う
  → 日本語 IME が KEYEVENTF_UNICODE を composition モードで処理し余計な文字が挿入されるため
- グローバルホットキーは `RegisterHotKey` API を使う（`tauri-plugin-global-shortcut` や `WH_KEYBOARD_LL` に戻さない）
  → plugin-global-shortcut では F4 が他アプリに漏れ、WH_KEYBOARD_LL は Windows 言語切替(Ctrl+Shift)と競合する
