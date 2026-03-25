---
name: codex
description: |
  Codex CLI（OpenAI）を使用して AIVoice プロジェクトのコードや設計について相談・レビューを行う。
  トリガー: "codex", "codexと相談", "codexに聞いて", "codexに調べて", "コードレビュー", "レビューして"
  使用場面: (1) Rust / Tauri の実装相談、(2) コードレビュー、(3) 設計の相談、(4) バグ調査、(5) 解消困難な問題の調査
---

# Codex（AIVoice プロジェクト専用）

Codex CLI を使用して AIVoice プロジェクト（Tauri 2 + Rust + Web UI）のコードレビュー・分析を実行するスキル。

## 実行コマンド

```bash
codex exec --full-auto --sandbox read-only --cd "E:/github/AIVoice" "<request>"
```

## プロンプトのルール

**重要**: Codex に渡すリクエストには、以下の指示を必ず末尾に含めること：

> 「確認や質問は不要です。具体的な提案・修正案・コード例まで自主的に出力してください。」

## パラメータ

| パラメータ | 説明 |
|---|---|
| `--full-auto` | 完全自動モードで実行 |
| `--sandbox read-only` | 読み取り専用サンドボックス（安全な分析用） |
| `--cd E:/github/AIVoice` | AIVoice プロジェクトディレクトリ固定 |
| `"<request>"` | 依頼内容（日本語可） |

## 実行手順

1. ユーザーから依頼内容を受け取る
2. プロジェクトディレクトリは `E:/github/AIVoice` を使用する
3. プロンプト末尾に「確認や質問は不要です。具体的な提案・修正案・コード例まで自主的に出力してください。」を追加する
4. 上記コマンド形式で Codex を実行する
5. 結果をユーザーに報告し、ファイルへ反映すべき変更があれば提案する
6. MVP 方針（docs/aivoice-mvp-plan.md）と矛盾する提案があれば指摘する
