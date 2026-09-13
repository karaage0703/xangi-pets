# 設計ドキュメント

## 概要

xangi-pets は Tauri 2 製のデスクトップアプリです。xangi が配信するイベントを pull 型 SSE で受け取り、ペットのアニメーションと吹き出しへ変換します。xangi 側にペットごとの callback URL を登録せず、複数のペットを接続できる構成です。

## アーキテクチャ

```text
xangi
  ├─ GET /api/events/stream ── SSE ──▶ xangi-pets Rust backend
  ├─ POST /api/sessions ◀───────────── ペット専用 Web session を作成
  └─ POST /api/pet/inbox ◀──────────── ペットからメッセージを送信
                                          │
                                          ▼
                                 embedded event server
                                   ├─ thread state aggregation
                                   ├─ /api/pet/state
                                   └─ /api/pet/bubbles
                                          │
                                          ▼
                                    Tauri webview
                                   Canvas + speech bubbles
```

Web Chat は別の通常ウィンドウに接続先 URL を表示します。リモートコンテンツにはペット webview 用の Tauri 権限を付与しません。

## コンポーネント

### Tauri アプリケーション（`src-tauri/src/`）

アプリの起動、透明・最前面ウィンドウ、メニューバーと通常アプリメニュー、接続プロファイル、通知、ウィンドウ位置とサイズを管理します。透明部分のクリックスルーと、画面外にあるペットを中央へ戻す処理もこの層が担当します。

### イベントサーバ（`src-tauri/crates/events-server/`）

xangi の SSE を購読し、turn 単位のイベントを thread ごとに集約します。集約結果をペット webview 向けの状態ストリームと吹き出しストリームとして配信します。また、ペットからの入力を専用 Web session に結び付けて xangi へ送信します。

### フロントエンド（`src/`）

Vite と vanilla JavaScript で構成します。Canvas 上のスプライト描画、状態に応じたアニメーション切り替え、吹き出しのページング、入力モーダル、ペット選択を担当します。

### 接続プロファイル（`src/lib/connection-profiles.js`）

xangi の名前、イベント API URL、Web UI の有無を保存します。プロファイル一覧は複数プロセスで共有し、各プロセスが選んだ接続先や表示設定は分離します。

### ウィンドウ配置（`src/lib/window-layout.js`）

ペットと吹き出しのスケールから必要なウィンドウ寸法を求めます。位置はプロファイルごとに保持し、起動時に表示領域が 48 ピクセル未満なら現在のモニター中央へ復帰させます。ユーザー操作でもメニューから明示的に中央へ戻せます。

## データフロー

### xangi の応答を表示する

1. Rust backend が `GET /api/events/stream` へ接続する
2. event server が `turn.started`、`message.delta`、`turn.complete` などを thread ごとに集約する
3. `/api/pet/state` が全体状態を、`/api/pet/bubbles` が吹き出しイベントを配信する
4. webview が状態に対応するスプライト行と吹き出しを描画する

状態は wire 上のイベントから consumer 側で導出します。

- `turn.started` 後、delta 前: `thinking`
- `message.delta` 受信後: `talking`
- `turn.complete` / `turn.aborted` 後: `idle`
- `agent.error` 後: `error`

### ペットからメッセージを送る

1. クリックまたは `t` キーで入力モーダルを開く
2. 初回送信時に `POST /api/sessions` でそのプロセス専用の Web session を作る
3. `appSessionId` を付けて `POST /api/pet/inbox` へ送る
4. 応答は通常の SSE 経路で受け取り、吹き出しへ表示する

専用 session を使うため、ブラウザや別デバイスの Web session に会話が混ざりません。

## 設計方針

### pull 型の接続

ペット側から xangi へ接続するため、ペットを増やしても xangi 側の callback 設定は不要です。切断時は指数 backoff で再接続します。

### 表示と通信の分離

Rust backend がイベントの接続・集約を担い、webview は集約済みデータの描画に集中します。wire protocol の詳細を UI へ持ち込みません。

### 接続先ごとの設定分離

キャラクター、サイズ、通知、位置は接続プロファイル単位で保持します。複数の xangi と複数ペットを同時に使っても設定が混ざらないようにします。

### デスクトップ操作を妨げない

ウィンドウは透明かつ最前面ですが、ペット以外の領域はクリックを透過します。ペットは自由に移動でき、画面外へ出ても自動判定またはメニュー操作で復帰できます。

### リモートコンテンツの権限分離

アプリ内 Web Chat は接続先が提供するリモートコンテンツです。ペット frontend の Tauri capability を Web Chat ウィンドウには与えません。URL は `http` / `https` に限定し、userinfo を拒否して query と fragment を除去します。

## 主要ファイル構成

```text
xangi-pets/
├── src/                         # Canvas UI、吹き出し、接続プロファイル
├── src-tauri/
│   ├── src/                     # Tauri lifecycle、メニュー、ウィンドウ制御
│   ├── crates/events-server/    # xangi SSE client、集約、pet inbox
│   ├── capabilities/            # Tauri capability
│   └── resources/default-pets/  # 同梱ペット
├── docs/                        # 利用・設計・イベント・インストール文書
├── scripts/                     # テストとライセンス生成
└── release-scripts/             # public release の準備・検証
```

イベントと endpoint の詳細は [イベント仕様](EVENTS.md) を参照してください。

## 拡張ポイント

### ペットを追加する

`pet.json` と `spritesheet.webp` を Codex `hatch-pet` 互換形式で配置します。アプリの再ビルドは不要です。

### xangi のイベントを追加する

wire schema、Rust の event model、thread aggregation、webview 向けの集約イベントを順に更新します。後方互換性と再接続時の重複通知を考慮します。

### 新しい OS へ配布する

CI のビルド成功だけで配布対象にせず、透明ウィンドウ、クリックスルー、メニュー、通知、複数モニターでの位置復帰を実機で確認します。
