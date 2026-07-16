# 言語設定・データ処理経路 Research / QA

## 仕様根拠

- OpenAI Audio Transcriptions API の `language` は任意の ISO-639-1 言語ヒント。Auto は省略し、日本語は `ja`、英語は `en` を送る。
- OpenAI Realtime transcription の `audio.input.transcription.language` も任意。公式OpenAIホストかつ現在明示対応している `gpt-realtime-whisper` にだけ送る。カスタム互換エンドポイントのRealtimeでは未検証パラメータを送らず、明示言語はBatchフォールバック時だけ送る。
- Realtime の `gpt-realtime-whisper` では prompt steering を追加しない。Batch prompt は選択言語に合わせ、Auto は `terms=`, `app=`, `window=` の短い構造化メタデータだけを使う。
- 参照: [Audio Transcriptions API](https://developers.openai.com/api/reference/resources/audio/subresources/transcriptions/methods/create)、[Realtime transcription](https://developers.openai.com/api/docs/guides/realtime-transcription)

## データ経路の表示方針

- URL はホスト名だけを表示し、userinfo・パス・クエリ・フラグメントを返さない。
- Realtime と Batch フォールバックはモデルを分けて表示する。
- Realtime は音声と言語ヒントを送信する。辞書語と前面アプリ情報は現在の実装では Batch／フォールバック時の prompt に限って送信され得る。
- Raw では Polish 経路を表示しない。Polish では Raw文字起こし、辞書語、カスタム指示、設定時の前面アプリ情報を表示する。
- APIキーがなければ「送信不可」と表示する。設定URLから端末内／クラウドを推定しない。
- 外部API側の保存・保持期間はアプリから断定せず、接続先プロバイダの契約・設定・ポリシーに依存すると表示する。
- 修正学習は明示確認方式で実装済み。Offでは修正候補を適用・保存・送信せず、Askではユーザーが保存した置換・語彙・文体例だけを次回以降に使う。置換は端末内で先に適用し、語彙は文字起こしのBatch prompt、文体例はPolishのfew-shotとして外部APIへ送信され得る。保存先は端末内の`corrections.json`であり、設定画面から全消去できる。

## 手動 QA

- 言語 Auto／日本語／英語を保存・再起動後に復元し、日本語、英語、混在発話を確認する。
- Raw／Polish、ライブ文字表示 ON／OFF、深いコンテキスト ON／OFF、辞書あり／なしで表示を確認する。
- userinfo・パス・クエリを含むカスタム Base URL でホスト以外が表示されないことを確認する。
- Realtime 失敗時に表示された Batch モデルでフォールバックすることを確認する。
- 修正学習をOff／Askで切り替え、Askで保存した置換・語彙・文体例が次の録音開始時点のスナップショットにだけ反映され、Undoおよび全消去後の再起動では反映されないことを確認する。

### 2026-07-15 Windows実機確認

- Tauri production buildで設定画面を開き、データ処理経路に接続先ホスト、Batchモデル、言語Auto、送信され得る内容、外部API側の保持に関する注意、端末内保存先が表示されることを確認した。
- 接続先は`api.openai.com`だけが表示され、URLのパス・クエリ・認証情報が露出しないことを確認した。
- 設定は保存せず閉じた。言語変更・カスタムBase URL・Realtimeフォールバックの実機確認は未実施。
