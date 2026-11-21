# イベント仕様

xangi-petsはxangiの`GET /api/events/stream`をSSEで購読します。Rustがイベントを集約し、ペットUIへ状態と吹き出しを配信します。

## 会話イベント（スキーマv2）

共通フィールド:

| フィールド | 型 | 内容 |
|---|---|---|
| `type` | string | イベント種別。必須 |
| `thread_id` | string | 会話の識別子。必須。例: `discord:<channel_id>`、`web:<session_id>` |
| `turn_id` | string | ユーザー入力から応答終了までの識別子。必須 |
| `thread_label` | string | 吹き出しに表示する会話名。省略時は短縮した`thread_id` |
| `ts` | number | 送信時刻（epoch ms）。省略時は受信側で補完 |
| `agent_id` | string | Agent識別子。指定時はAgent別の絞り込みに使用 |

受信時刻は`recv_ts`（epoch ms）として付与します。

| `type` | 内容 | 追加フィールド |
|---|---|---|
| `turn.started` | 応答開始 | `user_text`（任意） |
| `message.delta` | 応答の差分 | `text`（必須）、`full_text`（任意） |
| `turn.complete` | 正常終了 | `text`（最終応答、任意） |
| `turn.aborted` | 中断 | なし |
| `agent.error` | エラー | `message`（必須） |

```jsonl
{"type":"turn.started","thread_id":"web:example","turn_id":"turn-1"}
{"type":"message.delta","thread_id":"web:example","turn_id":"turn-1","text":"こんにちは。"}
{"type":"turn.complete","thread_id":"web:example","turn_id":"turn-1","text":"こんにちは。"}
```

状態は受信側で導出します。開始後は`thinking`、差分受信後は`talking`、完了・中断後は`idle`、エラー後は`error`です。

## 集約と通知

- threadごとに開く吹き出しは一つです。開始を受信していない差分でも吹き出しを開きます。
- 差分の`text`を追記し、完了時の`text`を最終応答にします。中断時は受信済みテキストを残します。
- Agent選択時は集約前に`agent_id`を比較し、ない場合はセッションAPIで会話の所有Agentを確認します。照合不能のターンは表示しません。Agent未選択は全体を購読します。
- 接続解除・切替時は`bubble.reset`で吹き出しを消し、状態を`idle`に戻します。切断時は指数backoffで再接続します。
- macOS通知は、有効化後かつ現在の接続で開始を観測したターンの完了・エラーに1回だけ出します。中断や再接続時の過去イベントには出しません。

## セッション監視

ONの接続先ごとに`GET /api/sessions/stream?mode=changes`を購読します。

- `sessions`: セッション一覧を全件置換
- `session_changes`: `upsert`を追加・更新し、`removed`のIDを削除
- `activity_snapshot`: 対象threadの実行状態を更新

表示種類はpets側の`session-filters.json`に`chat` / `web` / `scheduler`の配列で保存します。初期値は`["chat","web"]`で、自動実行はOFFです。空配列も有効です。変更は全接続先へ即時反映し、再起動後も保持します。

`scope=scheduler`または`executionOrigin=automation`は自動実行、その他の`platform=web`はWeb、それ以外はChatに分類します。`isActive=true`かつ`lifecycle!=closed`で、選択した種類に一致するセッションを表示します。非表示の行も保持し、フィルタ変更時に再評価します。

通信断だけではウィンドウを閉じず、再接続時の一覧で同期します。常駐ペットはフィルタの対象外です。

## ペットからの送信

初回送信時に`POST /api/sessions`でプロセス専用のWebセッションを作り、`POST /api/pet/inbox`に`appSessionId`を指定します。Agent選択時はセッション作成に`agentId`も渡します。セッションが見つからない場合は作成し直して1回再送します。

## 内蔵HTTP API

アプリ起動時にaxumサーバを起動します。既定は`0.0.0.0:7895`で、使用中なら後続ポートを試します。`XANGI_PET_BIND`と`XANGI_PET_PORT`で変更でき、実際のポートはメニューに表示します。

| エンドポイント | 内容 |
|---|---|
| `GET /health` | 接続診断用の状態 |
| `GET /api/pet/state` | 集約状態のSSE |
| `GET /api/pet/bubbles` | 吹き出しのSSE |
| `GET /api/pet/list` | 内蔵サーバの画像一覧 |
| `GET /api/pet/asset/:name/:file` | 内蔵サーバの画像取得 |

吹き出しイベント:

| `type` | 主なフィールド | 内容 |
|---|---|---|
| `bubble.snapshot` | `thread_id`, `turn_id`, `text` | 接続時の開いている吹き出し |
| `bubble.open` | `thread_id`, `turn_id` | 吹き出しを開く |
| `bubble.delta` | `thread_id`, `turn_id`, `text` | テキストを追記 |
| `bubble.close` | `thread_id`, `turn_id`, `last_message`, `aborted?` | 完了・中断 |
| `bubble.error` | `thread_id`, `turn_id`, `message` | エラー表示 |
| `bubble.reset` | なし | 吹き出しを全消去 |

内蔵APIに認証はありません。イベント表示はbest-effortで、順序の保証や失敗時の再送は行いません。

## ペット表情のエラー期限

ペット全体の表情は `talking` → `thinking` → 直近30秒の `error` → `idle` の順で選びます。エラーの期限は受信時の単調時計で測り、診断状態や吹き出しは保持します。状態SSEはイベント受信時と1秒ごとに再評価し、表情が変わったときだけ配信します。
