# 設計ドキュメント

## 概要

xangi-pets はTauri 2製のデスクトップアプリです。RustがxangiのSSEを購読し、WebViewが状態をアニメーションと吹き出しで表示します。ペットからxangiへ接続するため、xangi側でペットごとの接続設定は不要です。

## 構成

```text
xangi                            xangi-pets
/api/events/stream ── SSE ──▶ Rust: 会話の集約 ──▶ WebView: アニメーション・吹き出し
/api/sessions/stream ─ SSE ─▶ Rust: 実行状態 ──▶ セッションウィンドウ
/api/sessions ◀───────────── ペット専用Webセッションの作成
/api/pet/inbox ◀──────────── メッセージ送信
/api/workspace-file ───────▶ Agentのスプライト取得
```

- `src-tauri/src/`: 起動、メニュー、通知、設定保存、ウィンドウ制御
- `src-tauri/crates/events-server/`: SSE接続、会話の集約、セッション監視、画像取得、送信
- `src/`: Canvas描画、吹き出し、接続設定、入力画面

## 接続と設定

接続プロファイルは名前、URL、Web UIの有無、任意の`agentId`、workspace相対の`spriteDirectory`を保持します。プロファイル一覧はプロセス間で共有し、入力先は各プロセスで選びます。Agentの指示・AI設定・workspaceはxangi側で管理します。

見守る接続先は`enabled`をURL単位でまとめ、ONのURLごとにモニターを起動します。入力先には送信できる常駐ペット、それ以外には表示専用の常駐ペットを置きます。OFFにすると該当URLの購読とペットを停止します。

設定の保存先:

- アプリ設定ディレクトリの`connection-profiles.json`: 接続プロファイル
- 同ディレクトリの`pet-display.json`: 全ペット共通の倍率
- 同ディレクトリの`session-filters.json`: 全接続先共通のセッション表示種類
- WebViewのlocalStorage: 接続プロファイルごとの吹き出し・通知・常駐ペットの位置

## 会話と通知

Rustは`/api/events/stream`から受け取ったイベントをthread単位で集約し、`/api/pet/state`と`/api/pet/bubbles`でWebViewへ配信します。

- `turn.started`: `thinking`
- `message.delta`: `talking`
- `turn.complete` / `turn.aborted`: `idle`
- `agent.error`: `error`

Agent選択時は集約・通知より前に対象を絞ります。イベントの`agent_id`を比較し、ない場合はセッションAPIでthreadの所有Agentを照合します。照合結果はターン単位で保持し、照合できないターンは表示しません。

接続変更は送信と直列化し、切替前の購読終了後に会話・吹き出し・通知追跡をリセットします。通信断は指数backoffで再接続します。通知は有効化後かつ現在の接続で開始を観測したターンの完了・エラーを1回だけ対象にします。

## メッセージ送信

クリックまたは`t`キーで入力画面を開き、初回送信時に`POST /api/sessions`でプロセス専用のWebセッションを作ります。選択した`agentId`と、送信時の`appSessionId`で会話先を固定します。メッセージは`POST /api/pet/inbox`へ送り、応答はSSEで受け取ります。送信前にAgentの存在を確認し、不明なAgentへの送信はエラーにします。

ペット全体の表情は `talking` → `thinking` → 直近30秒の `error` → `idle` の順で選びます。エラーの期限は受信時の単調時計で測り、診断状態や吹き出しは保持します。状態SSEはイベント受信時と1秒ごとに再評価し、表情が変わったときだけ配信します。

## セッションペット

`session_monitor.rs`が`/api/sessions/stream?mode=changes`を購読します。`sessions`で全件を置き換え、`session_changes`と`activity_snapshot`で更新します。表示条件は`isActive=true`、`lifecycle!=closed`かつpets側で選択した種類との一致です。

種類は次の順で判定します。

1. `scope=scheduler`または`executionOrigin=automation`: 自動実行（`scheduler`）
2. `platform=web`: Web（`web`）
3. その他: Chat（`chat`）

フィルタはpets側で保存し、初期値はChat・WebがON、自動実行がOFFです。空配列なら全セッションペットを隠します。非表示の行も保持し、フィルタを戻すと再表示します。保存成功後にwatchチャネルで全モニターへ反映し、通信断中も直近の一覧を再評価します。新しい接続先には現在の設定を適用します。常駐ペットのイベントも集約・通知前に同じ種別で除外します。セッションAPIで取得した所有Agentと種別をturnごとに保持し、表示可否は現在の設定で再評価します。フィルタ変更時には既存の吹き出しと表情をクリアします。

`session_windows.rs`はURL・常駐/セッション種別・セッションIDでウィンドウを管理します。通信断だけでは閉じず、再接続時の全件一覧で同期します。表示専用ウィンドウは入力セッションや通知購読を作りません。画像の初回描画が終わってからラベルと一緒に表示します。読み込み中もcanvasを選択倍率の寸法に保ち、ラベルだけを先に表示しません。破棄時はタイマーとlistenerを解放します。

## 画像・サイズ・権限

画像は対象Agentのworkspaceから`/api/workspace-file`経由で取得し、失敗時は同梱の`xangi`を使います。相対パス、サイズ上限、WebP形式を検査し、検査済みbytesをWebViewへ渡します。配置と寸法は[使い方ガイド](usage.md#独自ペットを追加する)を参照してください。

倍率変更は保存・revision更新・全ウィンドウのリサイズ・通知を直列化し、古いrevisionを無視します。常駐・セッションペットは同じ描画寸法とクリック判定を使います。拡縮時は足元の中心を維持し、画面端で補正します。起動時に48ピクセル未満しか見えない常駐ペットはモニター中央へ戻します。

透明部分はクリックを背後へ通します。Web Chatは別ウィンドウで開き、ペット用のTauri権限を与えません。接続URLは`http` / `https`に限定し、userinfoを拒否してqueryとfragmentを除きます。

イベントのフィールドと内蔵APIは[イベント仕様](EVENTS.md)を参照してください。
