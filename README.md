# xangi-pets

[![CI Build](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml/badge.svg)](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml)
[![GitHub Release](https://img.shields.io/github/v/release/karaage0703/xangi-pets)](https://github.com/karaage0703/xangi-pets/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[English](README.en.md)

[xangi](https://github.com/karaage0703/xangi) の状態や応答をデスクトップで見守れる常駐ペットです。透明な最前面ウィンドウでアニメーションし、応答を吹き出しで表示します。ペット以外の透明部分はクリックを背後のアプリへ通します。

## 主な機能

- xangi の状態をアニメーションで、応答を吹き出しで表示する
- クリックまたは `t` キーで話しかけ、Web Chat も開ける
- 複数の xangi を同時に見守り、実行中セッションの担当Agentを自動表示する
- セッションの表示種類を「Chat・Web・自動実行」から選ぶ
- Agentのworkspaceに置いた `hatch-pet` 互換画像を使う（取得できない場合は同梱の `xangi`）
- ペットと吹き出しのサイズ、応答表示、macOS通知を調整する

## Quickstart

現在、GitHub Releases で配布している実機確認済みバイナリは macOS Apple Silicon 版です。

1. [Releases](https://github.com/karaage0703/xangi-pets/releases) から `xangi-pets_X.Y.Z_aarch64.dmg` をダウンロードする
2. アプリを `/Applications` へコピーし、Finder で右クリックして「開く」を選ぶ
3. 起動時に接続先を追加し、xangi の URL を入力する（同じ Mac の標準構成は `http://localhost:18888`）

xangi が別マシンにある場合は、LAN または Tailscale から到達できる URL を指定してください。Web UI を使わない xangi にも接続できます。

詳しい初回起動手順は [インストール手順](docs/INSTALL.md)、接続や操作方法は [使い方ガイド](docs/usage.md) を参照してください。

## 使い始める

ペットをクリックするか `t` キーを押すと、xangi へメッセージを送れます。ペットはドラッグで移動できます。

表示・接続先・通知・サイズはメニューバーまたは通常アプリメニューから操作できます。ペットが画面外へ出た場合は「ペットを画面中央に戻す」を選んでください。

接続先画面の「表示するセッション」で種類を選びます。初期値はChat・WebがON、自動実行がOFFです。設定は全接続先に適用され、常駐ペットは表示を続けます。詳しくは[セッションペット](docs/usage.md#セッションペット)を参照してください。

キーボード操作、複数起動、独自ペットの追加方法、トラブルシューティングは [使い方ガイド](docs/usage.md) にまとめています。

## 対応環境

- macOS Apple Silicon: 実機確認済み。GitHub Releases で `.dmg` を配布
- Windows x86_64 / Linux x86_64: CI でビルド確認済み。実機未確認のため現在は配布対象外

## ソースから開発する

Node.js 18 以降、Rust stable、各 OS の [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) が必要です。

```bash
npm ci
npm test
npm run tauri dev
```

開発・PR の手順は [CONTRIBUTING.md](CONTRIBUTING.md) を参照してください。

## ドキュメント

- [使い方ガイド](docs/usage.md) — 接続、メニュー、キー操作、複数起動、独自ペット、トラブルシューティング
- [インストール手順](docs/INSTALL.md) — macOS の初回起動、更新、アンインストール
- [設計ドキュメント](docs/design.md) — アーキテクチャ、コンポーネント、データフロー、設計方針
- [イベント仕様](docs/EVENTS.md) — xangi との SSE イベントと内蔵 API
- [CONTRIBUTING.md](CONTRIBUTING.md) — 開発環境とコントリビューション

## 関連プロジェクト

- [xangi](https://github.com/karaage0703/xangi) — AI エージェントをチャットや Web UI から利用するための本体
- [openai/skills hatch-pet](https://github.com/openai/skills/tree/main/skills/.curated/hatch-pet) — 互換スプライトの生成元

## License

Apache License 2.0。配布バンドルには Rust crate と npm package のサードパーティライセンス一覧を同梱しています。
