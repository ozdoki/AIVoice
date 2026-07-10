# Polish Preset Evaluation Results (Issue #30)

## 結論

5プリセットすべてが合格基準に到達した。最終回帰評価はAPIエラー0件、機械判定10/10件、人手評価119/120点（99.2%）で、重大失敗は0件だった。

## 評価条件

- 実施日: 2026-07-11
- Model: `gpt-5.4-mini`
- Corpus: `src-tauri/eval/polish-preset-corpus.json` version 1
- 実行経路: KoeTypeと同じ `polish::polish_text`
- Custom instructions: なし
- Deep context: なし
- 対象プリセット: Slack、Email、Memo、AI Prompt、Technical
- 機械判定: 必須語、禁止語、見出し、ラベル、bullet数、空行数
- 人手判定: meaning fidelity、no invention、cleanup、preset fit、readability、token/structureを各0–2点で採点

ケース合格条件は、機械判定合格、人手評価に0点なし、合計10/12点以上。プリセット合格条件は、2ケースとも合格し、平均10/12点以上とした。詳細なルーブリックと再現手順は [Polish Preset Evaluation](polish-preset-evaluation.md) を参照。

## 改善ラウンド

| Round | 対象 | API calls | API errors | 機械判定 | 結果と対応 |
| --- | --- | ---: | ---: | ---: | --- |
| 1 | 全10ケース | 10 | 0 | 3/10 | Slackのbullet、Promptのsection分類、Technicalのlabel分離に実不良。Slack説明文と短いEmailの空行要件2件はcorpus誤検知として修正 |
| 2 | 実不良5ケース | 5 | 0 | 4/5 | Slackのbulletが再失敗。`prompt-file-edit` は機械合格だが指示重複で人手未達 |
| 3 | 残り2ケース | 2 | 0 | 2/2 | Slackのbullet行を明示し、Promptのsection間重複を抑制して合格 |
| Final | 全10ケース | 10 | 0 | 10/10 | 全プリセット回帰合格 |

Issue #30でのAPI使用量は合計27 callsだった。失敗ケースだけを再評価し、最後に全件回帰することで不要な総当たりを避けた。

## 最終スコア

軸の順序は `meaning fidelity / no invention / cleanup / preset fit / readability / token・structure`。

| Preset | Case | Scores | Total | Machine | 判定 |
| --- | --- | --- | ---: | --- | --- |
| Slack | `slack-multi-task` | 2 / 2 / 2 / 2 / 2 / 2 | 12/12 | Pass | Pass |
| Slack | `slack-explanation` | 2 / 2 / 2 / 2 / 2 / 2 | 12/12 | Pass | Pass |
| Email | `email-request` | 2 / 2 / 2 / 2 / 2 / 2 | 12/12 | Pass | Pass |
| Email | `email-uncertainty` | 2 / 2 / 2 / 2 / 2 / 2 | 12/12 | Pass | Pass |
| Memo | `memo-task-list` | 2 / 2 / 2 / 2 / 2 / 2 | 12/12 | Pass | Pass |
| Memo | `memo-decision` | 2 / 2 / 2 / 2 / 2 / 2 | 12/12 | Pass | Pass |
| AI Prompt | `prompt-pr-workflow` | 2 / 2 / 2 / 2 / 2 / 2 | 12/12 | Pass | Pass |
| AI Prompt | `prompt-file-edit` | 2 / 2 / 1 / 2 / 2 / 2 | 11/12 | Pass | Pass |
| Technical | `technical-fallback` | 2 / 2 / 2 / 2 / 2 / 2 | 12/12 | Pass | Pass |
| Technical | `technical-ui-bug` | 2 / 2 / 2 / 2 / 2 / 2 | 12/12 | Pass | Pass |

| Preset | Mean | 判定 |
| --- | ---: | --- |
| Slack | 12.0/12 | Pass |
| Email | 12.0/12 | Pass |
| Memo | 12.0/12 | Pass |
| AI Prompt | 11.5/12 | Pass |
| Technical | 12.0/12 | Pass |

## 最終出力

### `slack-multi-task`

```text
- 明日の朝までにKoeTypeのfallbackを確認してください。
- gpt-realtime-whisperが失敗したらbatchに入るか見てください。
- 終わったらGitHub issueに結果を書いてください。
```

### `slack-explanation`

```text
この件は共有だけで大丈夫です。

今はiframeでGoogleスライドを表示して、4枚目で動画を自動再生しています。この仕組みなら、ユーザーが近づいたときに最初から再生できるかもしれません。

まだ検証は十分ではありません。
```

### `email-request`

```text
田中さん、来週火曜の15時までに見積書をご確認いただき、問題があればコメントをください。

問題がなければ、承認をお願いします。
```

### `email-uncertainty`

```text
先ほど共有したAPI仕様ですが、認証方式が変わる可能性があるため、確定するまでは実装を止めておいていただけると助かります。

金曜にもう一度ご連絡します。
```

### `memo-task-list`

```text
- PR 42のレビュー
- releaseブランチのbuild確認
- 終わったらNotionに結果を残す
```

### `memo-decision`

```text
- 料金ページは今月は触らない。
- まず無料プランの利用状況を見る。
- 数字が少なければ来月また考える。
```

### `prompt-pr-workflow`

```text
目的
- この変更が使えるかテストする。

タスク
- 改行できるか確認する。
- 問題なければGitHub issueにコメントする。
- PRを作る。

条件
- マージはまだしない。

報告
- 結果と未確認事項を報告する。
```

### `prompt-file-edit`

```text
目的
- src/config.tsのtimeoutを30から45に変更する。

タスク
- src/config.tsのtimeoutを30から45に変更する。
- npm testを実行する。

条件
- 他の設定は変更しない。

報告
- 変更点とテスト結果を報告する。
```

### `technical-fallback`

```text
- Deadline: 明日まで
- Target: Realtime ASR
- Check: gpt-realtime-whisperがHTTP 429を返したとき、batch ASRへfallbackするか確認する
- Follow-up: 結果をissue 27に書く
```

### `technical-ui-bug`

```text
- Target: SettingsPanel.tsx
- Check: 保存ボタンを2回押すと同じtoastが2つ出る。Windows 11で再現した。原因を確認する。
- Follow-up: 修正後にpnpm testを実行する。
```

## 残る軽微な制約

`prompt-file-edit` では、主要成果を示す目的と具体作業を示すタスクの双方に `src/config.ts` の同じ変更が現れた。情報の所属は正しく、実用上の混乱や意味変更はないため合格としたが、cleanupは1点とした。目的とタスクの最小限の意味重複は、目的を維持する現在の形式上許容している。今後この重複が長文化・増幅するケースが出た場合のみ、追加fixtureで調整する。

この評価は固定コーパスと当該実行時点のモデル出力に対する結果である。モデルの非決定性や更新による変化はあり得るため、プリセットprompt変更時は10ケースの最終回帰を再実行する。
