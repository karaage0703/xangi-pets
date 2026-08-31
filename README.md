# xangi-pets

[![CI Build](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml/badge.svg)](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml)
[![GitHub Release](https://img.shields.io/github/v/release/karaage0703/xangi-pets)](https://github.com/karaage0703/xangi-pets/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[English](README.en.md)

xangi用のデスクトップ常駐ペット。透明な最前面ウィンドウでアニメーションし、xangiの応答を吹き出しで表示する。ペット以外の透明部分はクリックを背後のアプリへ通す。

## 主な機能

- xangiの `idle / thinking / talking / error` に合わせたアニメーション
- 複数会話の吹き出し表示と、長文の4秒ごとの自動ページ送り
- ペットのクリックまたは `t` キーでxangiへメッセージ送信
- メニューバー常駐と通常アプリメニュー。ペットを隠しても、再表示・話しかける・Web Chat・接続先変更へアクセス可能
- Web Chatをxangi-pets内の通常ウィンドウで表示（必要なら既定ブラウザでも開ける）
- xangiとの接続状態（接続中・接続済み・再接続中）と、内蔵サーバのポートを両方のメニューで確認
- 通常応答と完了通知の吹き出しを個別にON/OFF。会話中は表示せず、完了内容だけ表示する運用にも対応
- 任意で新規turnの完了・エラーをmacOSのシステム通知（初回起動では通知許可を要求しない）
- ペットと吹き出しをそれぞれ5段階で拡大
- Codex `hatch-pet` 互換スプライトと、公開用に制作した同梱ペット `xangi`
- 動作確認済みのmacOS Apple Silicon向け配布バイナリ

## すぐに使う

1. macOS Apple Siliconで、[Releases](https://github.com/karaage0703/xangi-pets/releases) から`.dmg`をダウンロード
2. インストールして起動（初回の警告対応は [インストール手順](docs/INSTALL.md) を参照）
3. 表示された入力欄へxangiのWeb Chat URLを指定。通常は `http://localhost:18888`

xangiが別マシンの場合は、LANまたはTailscaleから到達できるWeb Chat URLを指定する。あとから変更するにはペットへフォーカスして `x` キーを押す。

## メニューバーと通常アプリメニュー

起動中はメニューバーにxangi-petsが常駐する。同じ主要操作を、xangi-petsがアクティブなときの通常アプリメニューからも実行できる。メニューバーのアイコンがほかの常駐アプリに隠れた場合も、Dockからxangi-petsを選べば通常アプリメニューを使える。

- ペットを表示 / 隠す
- xangiに話しかける（ペットを表示してフォーカスした後、既存の入力モーダルを開く）
- Web Chatをアプリで開く（設定済みのxangi URLを通常のTauriウィンドウに表示）
- Web Chatをブラウザで開く
- xangi URLを設定
- 通常応答表示・完了通知表示・システム通知の個別ON/OFF
- ヘルプ、終了

接続済み表示はSSE handshakeが成功した場合だけになる。xangiが停止すると再接続中へ変わり、復帰後は自動的に接続済みへ戻る。通知をONにした場合も、ON以後に開始したturnの完了またはエラーだけを1回通知し、再接続時の過去イベントは通知しない。ペットから最初に話しかけたときは、そのxangi-petsプロセス専用の新しいWebセッションを作る。同じ起動中の次のメッセージはそのセッションを継続し、既存のブラウザや別デバイスのWebセッションへ混ざらない。

接続先は `http://` または `https://` のみ受け付け、URL内のuserinfoは拒否する。queryとfragmentは保存・表示・アプリ内表示・ブラウザ起動の前に除去する。macOS版はlocalhost・LAN・Tailscaleで一般的なHTTPのWeb ChatをWKWebViewへ表示できるよう、埋め込みWebコンテンツだけにApp Transport Securityの例外を適用する。アプリ内Web Chatはリモートコンテンツとして表示し、ペット側のTauri権限を付与しない。

## 設計

- **生成**: 既存の Codex `hatch-pet` で作ったスプライトを流用（`~/.codex/pets/<name>/`）
- **表示**: Tauri 2 で透明ウィンドウ + always-on-top。Canvas で 8x9 アトラスをループ描画
- **配信（pull 型 SSE）**: Tauri アプリが起動時に **xangi の `GET /api/events/stream`** に SSE で接続し、turn ライフサイクルイベントを受信する。xangi 側は固定 1 ポート 1 URL で配信、ペットを N 台繋いでも xangi 側の設定変更は不要。配布物は単一バイナリ
- **状態連動**: 受信したイベントを内蔵 axum サーバの集約 bus に流し、SSE (`/api/pet/state`) で `idle / thinking / talking / error` を webview にプッシュ
- **吹き出し表示**: SSE (`/api/pet/bubbles`) で集約済み bubble.* イベントを受信し、ペット上に発話バブルを描画

```
xangi (web-chat :18888) ──/api/events/stream (SSE)──▶ xangi-pets (Tauri)
                                                       ├─ pull client ─▶ axum bus
                                                       ├─ pet webview ─▶ Canvas + bubble
                                                       └─ Web Chat window ─▶ configured xangi URL
```

xangi 側 URL は **起動時に prompt** で入力（`x` キーで再設定可）。env `XANGI_URL` でも初期値を渡せる。設定後は localStorage と Tauri 側 OnceLock の両方に保存され、再起動で自動再接続。

イベントスキーマと SSE エンドポイントの詳細は [`docs/EVENTS.md`](docs/EVENTS.md)。

### スプライト素材の置き場所

デフォルトで、公開用に制作したオリジナルサンプル **`xangi`** が同梱されているので、何も用意しなくても起動するだけでペットが出る。個人キャラクターの素材は公開配布物に含めない。自作・利用許諾のあるスプライトを使いたい場合は以下に置く（**優先順**で探索）：

```
~/.xangi/pets/<pet-name>/   # 優先（xangi 専用）
~/.codex/pets/<pet-name>/   # フォールバック（Codex hatch-pet と共有）
<アプリ同梱>/xangi/          # 最終フォールバック（バンドル済み default、上書き可）
└── pet.json                 # メタ情報（id / displayName など）
└── spritesheet.webp         # スプライトシート（hatch-pet 互換）
```

- 同じ名前が複数あれば `~/.xangi/pets/` が勝つ。同梱 `xangi` を差し替えたいときは `~/.xangi/pets/xangi/` に上書き素材を置けばそっちが採用される
- ペットの選択は **起動時のピッカー** で行う（複数あれば一覧から選ぶ、1 つだけならそれが採用される）。選択は `xangi-pets:name` (localStorage) に保存される
- 起動後にペットを切り替えたいときはウィンドウ上で `c` キー
- 探索パスを完全に上書きしたいときは `XANGI_PET_DIR` 環境変数（指定したら *そこだけ* 見る、同梱 default も無視される）
- それでも素材が見つからなかった場合のみ、セットアップ手順がウィンドウ内に表示される

### スプライト形式（hatch-pet 互換）

- 1536×1872 の 8列×9行 アトラス（192×208セル, 透明 webp）
- 行 = アニメーション種別（[`references/animation-rows.md`](https://github.com/openai/skills/blob/main/skills/.curated/hatch-pet/references/animation-rows.md) の順）
  - 0: idle / 1: running-right / 2: running-left / 3: waving / 4: jumping
  - 5: failed / 6: waiting / 7: running / 8: review

素材を作るには Codex の [hatch-pet スキル](https://github.com/openai/skills/tree/main/skills/.curated/hatch-pet) を使うのが一番ラク（Codex CLI で `/pet` 系を辿ると `~/.codex/pets/<name>/` に自動配置される）。自作する場合はアトラス仕様に従えばよい。

### xangi 状態 → 行マッピング

| xangi state | 行 |
|---|---|
| `idle` | 0 (idle) |
| `thinking` | 8 (review) |
| `talking` | 3 (waving) |
| `error` | 5 (failed) |

## ディレクトリ構成

```
xangi-pets/
├── src/                   # フロント (Vite + vanilla JS)
│   ├── index.html
│   ├── main.js            # Canvas 描画 + SSE 受信 + 状態切り替え
│   ├── styles.css
│   └── lib/bubble.js      # 吹き出し UI のステートマシン
├── src-tauri/             # Tauri (Rust)
│   ├── Cargo.toml
│   ├── tauri.conf.json    # 透明 + always-on-top + skipTaskbar
│   ├── capabilities/
│   ├── src/               # Tauri setup + ウィンドウ制御
│   └── crates/
│       └── events-server/ # xangi SSE購読 + bubble集約 + 素材配信
├── docs/
│   ├── INSTALL.md         # OS別インストール
│   └── EVENTS.md          # イベントスキーマ仕様
├── scripts/               # テスト、ライセンス生成
└── package.json
```

## キー操作

| キー | 動作 |
|---|---|
| `c` | ペットを選び直す（ピッカー表示、Esc キャンセル） |
| `b` | 吹き出しサイズを cycle（1.0 → 1.3 → 1.6 → 2.0 → 2.5）。文字・パディング・ウィンドウが追従 |
| `p` | キャラサイズを cycle（0.5 → 0.8 → 1.1 → 1.5 → 2.0）。ウィンドウとヒットテストも追従 |
| `x` | **xangi の URL（pull 元）を設定**。空欄で接続解除。例: `http://localhost:18888` |
| `t` | **xangi にテキストを送信**。1 行入力モーダルを開き、`POST /api/pet/inbox` 経由で xangi に投げる。応答は通常通り SSE pull でバブル表示される |
| `h` / `?` | ヘルプを表示・非表示 |
| ペットをクリック | 上の `t` と同じ。ドラッグ移動は従来通り（5px 以下の動きでクリック判定） |
| 吹き出しをクリック | 読み終えた吹き出しを閉じる |

クリックスルーはペット領域以外で自動的に有効になる（透明エリアは下のアプリにクリックが届く）。

`1`〜`9` は開発用のアニメーション確認キーとして利用できる。

### xangi 側の要件

`t` キー / クリック送信を使うには xangi が `POST /api/pet/inbox` を受け付ける必要がある（xangi >= v0.27 想定）。

認証は xangi 側で自動判定:

- 同一マシン (loopback) / LAN / Tailscale から繋ぐ → そのまま通る（設定ゼロ）
- グローバル IP (Cloudflare Tunnel 等で xangi を公開してる場合) → xangi 側で `XANGI_PET_INBOX_TOKEN` を設定し、pet 側は同じ値を環境変数 `XANGI_PET_INBOX_TOKEN` で起動時に渡す

## サイズ変更

### キャラサイズ

`p` キーで 5 段階を cycle（0.5 / 0.8 / 1.1 / 1.5 / 2.0）。0.5 が default、それ以上はデモ・登壇向け（プロジェクタや大画面で見せたいとき 1.5 〜 2.0 が映える）。選択は `xangi-pets:scale` (localStorage) に保存される（`VITE_XANGI_PET_SCALE` で初期値も指定可）。変更はリロードなしで即反映される。

キャラサイズが変わると:

- ウィンドウ幅・高さがキャラと吹き出しの両方を収めるよう自動でリサイズ
- Rust 側のクリックスルー判定（ヒットテスト矩形）も `set_pet_size` で同期 → クリック領域がズレない
- ウィンドウ位置は **bottom-center 維持**（ペットの足元を固定）でリサイズされるので、ペットがその場でジャンプしない

### 吹き出しサイズ

`b` キーで 5 段階を cycle（1.0 / 1.3 / 1.6 / 2.0 / 2.5）。1.0 が default、それ以上は大きく寄り。文字（`font-size`）、内側余白、しっぽも `--bubble-scale` CSS 変数で一括スケール。選択は `xangi-pets:bubble-scale` (localStorage) に保存される。ウィンドウ高さ・幅も scale に追従して伸びるので、文字が大きくなっても見切れない。`VITE_XANGI_PET_BUBBLE_SCALE` で初期値を上書きできる。

吹き出し変更もリロードなしで即反映される。

長文は1ページ4行で表示し、応答完了後に4秒間隔で次ページへ進む。最終ページまで表示したあと先頭へ戻る。ストリーミング中は最新部分を表示し、応答完了時に先頭ページへ戻る。

ウィンドウのベースサイズは `src-tauri/tauri.conf.json` の `width` / `height`（280×200、pet=0.5 + bubble=1.0 を前提に算出）。実際のウィンドウサイズは起動時に `pet-scale` × `bubble-scale` から動的計算される。

## 複数起動

xangi-pets は同じマシンで複数起動できる（異なるキャラ・吹き出しサイズで横並びにできる）。

```bash
# 一度ビルドして .app を作る
npm run tauri build

# Mac で複数立ち上げ
open -n -a /Applications/xangi-pets.app
open -n -a /Applications/xangi-pets.app
```

仕組み:

- 内蔵 HTTP サーバの port は **7895 → 7896 → … と auto-shift**（`PORT_AUTOSHIFT_TRIES=10`）するので、port 衝突しない
- 各プロセスのメニューバーtooltipと状態行に実際のbound portを表示するため、複数インスタンスを識別できる
- 接続後のlocalStorageキーは **接続プロファイルIDをnamespaceに使う**。各起動slotは選択したプロファイルを記憶し、キャラ・サイズ・通知設定を接続先ごとに分離する
- 既存ユーザの `xangi-pets:name` 等は legacy fallback として読み込まれる（破壊的変更なし）

起動時の接続先ピッカーで、xangiの名前・イベントAPI URL・Web UIの有無を「接続プロファイル」として保存できる。「＋ 新しい接続先を追加」を選ぶと既存設定を上書きせずに登録する。接続プロファイル一覧はTauriの共通設定へ保存され、内蔵サーバのポートが異なる複数のペットから同じ一覧を選べる。複数起動した新しいペットで保存済みプロファイルを選ぶと、以後はそのペットが同じxangiへ自動再接続する。キャラ・サイズ・通知設定もプロファイル単位で保持される。

Web UIなしのxangiは「Web UIも利用する」をOFFにする。イベント表示とペットからの入力は維持し、Web Chatメニューだけが無効になる。xangi側は `XANGI_EVENTS_SERVER_ENABLED=true` のheadless companion APIを使う。

`npm run tauri dev` の多重起動は **vite dev サーバ（:1420 strictPort）が衝突するので非対応**。複数起動はビルド版でやる。

## ダウンロードして使う（配布バイナリ）

現在の配布・動作確認対象はmacOS Apple Siliconのみ。[Releases](https://github.com/karaage0703/xangi-pets/releases) から`xangi-pets_X.Y.Z_aarch64.dmg`を取得できる。ad-hoc署名のため、初回は右クリックから開く。

Windows x86_64とLinux x86_64はGitHub Actionsでパッケージのビルドまで確認しているが、実機動作は未確認。そのためGitHub Releaseでは配布していない。

**初回起動時のセキュリティ警告対応** + **ペット素材の置き場所**は [docs/INSTALL.md](docs/INSTALL.md) を参照。BOOTH 配布版もこの手順で動く。

## 開発

必要環境はNode.js 18以降、Rust stable、各OSの[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/)。

```bash
npm ci
npm test
npm run tauri dev
```

同梱 `xangi` が自動で使われるため、スプライト配置は不要。独自ペットを使う場合だけ「スプライト素材の置き場所」を参照する。開発・PRの手順は [CONTRIBUTING.md](CONTRIBUTING.md) を参照。

## ブラウザ単体での起動（Tauri 不要）

Tauri アプリを起動できない環境でもフロントだけ動かしたい場合：

```bash
# 1. 内蔵サーバを単独で起動（ターミナル A）
cargo run --release --example standalone --manifest-path src-tauri/crates/events-server/Cargo.toml

# 2. Vite dev サーバを起動（ターミナル B）
npm run dev

# 3. ブラウザで http://localhost:1420/ を開く
```

ブラウザモードでは drag / 自動リサイズ / クリックスルーは動かない（Tauri 専用機能、try/catch で吸収）が、ペット描画・バブル表示・SSE 受信はそのまま動く。

## ビルド（Mac .dmg）

```bash
npm run tauri build
# 成果物: src-tauri/target/release/bundle/dmg/xangi-pets_X.Y.Z_aarch64.dmg
```

## 対応状況

- macOS Apple Siliconで実機動作を確認し、GitHub Releaseで配布
- Windows x86_64、Linux x86_64はGitHub Actionsでビルド確認のみ（実機未確認・配布対象外）
- 複数ペット素材の選択と複数ウィンドウ起動に対応
- xangiとの通信はpull型SSE。接続先URLごとに独立して動作

## ライセンス

Apache License 2.0（`hatch-pet` 本家と互換）。

変更提案は [CONTRIBUTING.md](CONTRIBUTING.md) を参照。

### サードパーティライセンス

配布バンドル（.dmg / .app）の `Resources/` 直下に以下を同梱する：

- `LICENSE` — 本プロジェクトの Apache License 2.0 全文
- `THIRD_PARTY_LICENSES.html` — 同梱される Rust crate 全部の attribution（Tauri 2、axum、reqwest など）。`cargo about` で自動生成
- `THIRD_PARTY_NPM_LICENSES.html` — 同梱される本番 npm package 全部のライセンス。インストール済みpackageから自動生成

依存 crate を追加・更新したら `scripts/gen-licenses.sh` を流して再生成し、`src-tauri/THIRD_PARTY_LICENSES.html` を commit する：

```bash
cargo install cargo-about --locked --features cli   # 初回のみ
./scripts/gen-licenses.sh
```

本番npm依存を追加・更新したら、`npm ci`の後にライセンス一覧も再生成する：

```bash
npm run licenses:npm
```

## 参考

- [openai/skills hatch-pet](https://github.com/openai/skills/tree/main/skills/.curated/hatch-pet)
- [Codex app Settings (Pets)](https://developers.openai.com/codex/app/settings)
- [Tauri 2 docs](https://tauri.app/)
