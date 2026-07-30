# Event Schema

xangi-events サーバが扱うイベントの形式。[xangiの外部イベントストリーム](https://github.com/karaage0703/xangi/blob/main/docs/events.md)のスキーマv2に追従する。
状態ラベル (`thinking` / `talking` / `idle`) は wire には流さず、consumer 側で派生する。

> 実装は **Tauri アプリ (`xangi-pets`) に内蔵された axum サーバ**。
> 起動: `npm run tauri dev` または配布バイナリの実行で自動的に
> `0.0.0.0:7895` で listen する。`XANGI_PET_PORT` / `XANGI_PET_BIND` で変更可。

## Common fields

| field | type | notes |
|---|---|---|
| `type` | string | 下記の event types のいずれか。必須 |
| `thread_id` | string | 並列会話の識別子。プラットフォーム不問の opaque 文字列（`discord:<channel_id>` / `slack:<channel_id>` / `web:<session_id>` 等）。必須 |
| `thread_label` | string | 任意。バブル UI に表示する人間可読名（例 Discord チャンネル名 `"#general"`）。送られなければ短縮した `thread_id` で代替 |
| `turn_id` | string | 1 ターン = ユーザーメッセージ→応答完了。**全イベントで必須** |
| `ts` | number | 送信側 epoch ms。省略時はサーバが現在時刻を入れる |

サーバは受信時に `recv_ts` (number, epoch ms) を付与してから再配信する。

## Event types (v2)

| type | trigger | extra fields |
|---|---|---|
| `turn.started` | ユーザーメッセージ受信時 | `user_text` (任意) |
| `message.delta` | 応答ストリームのデルタ毎 | `text` (必須), `full_text` (任意) |
| `turn.complete` | ターン正常終了 | `text` (最終応答, 任意) |
| `turn.aborted` | ユーザー操作で cancel | — |
| `agent.error` | 例外発生 | `message` (必須) |

> 旧スキーマで存在した `agent.thinking` / `agent.talking` / `agent.idle` は v2 で削除。
> consumer 側で派生する：
> - `turn.started` 受信後 `message.delta` まだなし → "thinking"
> - `message.delta` が 1 回でも来た → "talking"
> - `turn.complete` または `turn.aborted` 受信後 → "idle"
> - `agent.error` 受信後 → "error"

## 典型シーケンス

正常完了：
```jsonl
{"type":"turn.started",  "thread_id":"discord:T","turn_id":"u1"}
{"type":"message.delta", "thread_id":"discord:T","turn_id":"u1","text":"そうそう。"}
{"type":"message.delta", "thread_id":"discord:T","turn_id":"u1","text":"かなり実用的"}
{"type":"turn.complete", "thread_id":"discord:T","turn_id":"u1","text":"そうそう。かなり実用的"}
```

cancel：
```jsonl
{"type":"turn.started",  "thread_id":"discord:T","turn_id":"u2"}
{"type":"message.delta", "thread_id":"discord:T","turn_id":"u2","text":"途中まで…"}
{"type":"turn.aborted",  "thread_id":"discord:T","turn_id":"u2"}
```

エラー：
```jsonl
{"type":"turn.started", "thread_id":"discord:T","turn_id":"u3"}
{"type":"agent.error",  "thread_id":"discord:T","turn_id":"u3","message":"timeout"}
```

並列に別チャンネルで会話が走っていれば、`thread_id` を分けて同時に流せる。
サーバ側は thread ごとに状態を集約する。

## Endpoints

イベントは pet → xangi の pull 型 SSE で取得する。pet 側の embedded HTTP
サーバは下記の集約済みエンドポイントだけを公開する（`POST /events` /
`GET /events` の生イベント受信路は廃止済）。

### Pull source (xangi 側)

Pet は xangi の `GET /api/events/stream` (SSE) を購読する。詳細は
[xangiの `docs/events.md`](https://github.com/karaage0703/xangi/blob/main/docs/events.md) を参照。

Tauri側はSSE handshake成功時だけ接続済みとする。stream切断または接続エラー後は再接続中とし、指数backoffで自動再接続する。

macOS通知を有効にした場合は、通知有効後かつ現在の接続中に `turn.started` を観測したturnだけを追跡する。そのturnの `turn.complete` または `agent.error` を最初に受信したときだけ通知する。再接続時に追跡中turnを破棄するため、過去イベントや終端イベント単独では通知しない。`turn.aborted` も通知しない。

ペットからの送信では、xangi-petsの各プロセスが最初の送信時に `POST /api/sessions` で専用Webセッションを作り、そのIDを `POST /api/pet/inbox` の `appSessionId` に必ず指定する。同じ起動中はそのセッションを継続し、xangi再起動などでセッションが見つからない場合だけ新規作成して1回再送する。`appSessionId`省略時に最新Webセッションを再利用するxangiの後方互換動作には依存しない。

### `GET /api/pet/bubbles` (SSE)

ペット UI 向けの **集約済み** ストリーム。クライアントは event 解釈不要で描画できる。

| sub-type | fields | 意味 |
|---|---|---|
| `bubble.snapshot` | `thread_id`, `turn_id`, `text` | 接続時、現在 open 中の bubble |
| `bubble.open` | `thread_id`, `turn_id` | 新規 bubble |
| `bubble.delta` | `thread_id`, `turn_id`, `text` | text を **追記** する差分 |
| `bubble.close` | `thread_id`, `turn_id`, `last_message`, `aborted?` | bubble を閉じる。`turn.aborted` 由来なら `aborted: true` |
| `bubble.error` | `thread_id`, `turn_id`, `message` | エラー表示 |

### `GET /api/pet/state` (SSE)

サーバ側で thread store から派生した単一の集約状態（`idle`/`thinking`/`talking`/`error`）。
スプライト行切替などペット表情の用途。

## サーバ側の thread 集約ルール

- `turn.started` で bubble open、状態 → "thinking"
- `message.delta` で text 連結、状態 → "talking"
  - `turn.started` を受け取ってない turn_id でも、デルタが来たら implicit に open（dropped data 回避）
- `turn.complete` で bubble close、`text`（または旧式の `last_message`）を最終テキストに、状態 → "idle"
- `turn.aborted` で bubble close、それまで貯めた text を `last_message` に、`aborted: true` 付与、状態 → "idle"
- `agent.error` で bubble.error、状態 → "error"
- 一度に open できる bubble は thread_id ごとに 1 つだけ

## 注意

- イベントは順序保証なし。送信側が時系列で投げる責任を負う
- 失敗時の再送なし。Pet 表示は best-effort
- 認証なし。Tailscale 内 / localhost 想定
- 接続先URLは `http` / `https` のみ。userinfoを拒否し、query / fragmentを除去してbase URLとして扱う
