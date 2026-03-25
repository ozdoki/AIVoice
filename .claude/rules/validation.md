---
paths:
  - "src-tauri/**/*.rs"
  - "src-tauri/Cargo.toml"
  - "src/**/*.ts"
  - "src/**/*.tsx"
  - "package.json"
---

# 検証ルール

- 変更対象に応じて `cargo test` / `cargo check` / `npm run lint` / `npm run test` / `npm run build` など利用可能な検証を実行する。
- 検証手段が未整備でも、最低限の確認方法を探したうえで未実行理由を明示する。
- 未検証のまま「動作する」と断定せず、確認した事実だけを報告する。
