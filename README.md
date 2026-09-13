# xangi-pets

[![CI Build](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml/badge.svg)](https://github.com/karaage0703/xangi-pets/actions/workflows/ci-build.yml)
[![GitHub Release](https://img.shields.io/github/v/release/karaage0703/xangi-pets)](https://github.com/karaage0703/xangi-pets/releases)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[English](README.en.md)

[xangi](https://github.com/karaage0703/xangi) の状態や応答をデスクトップで見守れる常駐ペットです。透明な最前面ウィンドウでアニメーションし、応答を吹き出しで表示します。ペット以外の透明部分はクリックを背後のアプリへ通します。

## 主な機能

- xangi の `idle / thinking / talking / error` に合わせてペットが動く
- 複数の会話を吹き出しで表示し、長文は自動でページ送りする
- ペットのクリックまたは `t` キーから xangi に話しかける
- メニューバーや通常アプリメニューから表示、接続先、通知を操作する
- xangi の Web Chat をアプリ内または既定ブラウザで開く
- 通常応答、完了メッセージ、macOS システム通知を個別に切り替える
- ペットと吹き出しをそれぞれ 5 段階で拡大する
- 複数の接続先と複数ペットを使い分ける
- Codex `hatch-pet` 互換スプライトと同梱ペット `xangi` を利用する

## Quickstart

現在、GitHub Releases で配布している実機確認済みバイナリは macOS Apple Silicon 版です。

1. [Releases](https://github.com/karaage0703/xangi-pets/releases) から `xangi-pets_X.Y.Z_aarch64.dmg` をダウンロードする
2. アプリを `/Applications` へコピーし、Finder で右クリックして「開く」を選ぶ
3. 起動時に接続先を追加し、xangi の URL を入力する（同じ Mac の標準構成は `http://localhost:18888`）

xangi が別マシンにある場合は、LAN または Tailscale から到達できる URL を指定してください。Web UI を使わない xangi にも接続できます。

詳しい初回起動手順は [インストール手順](docs/INSTALL.md)、接続や操作方法は [使い方ガイド](docs/usage.md) を参照してください。

## 使い始める

ペットをクリックするか `t` キーを押すと、xangi へメッセージを送れます。ペットはドラッグで移動できます。

起動中はメニューバーに常駐し、次の操作をいつでも呼び出せます。

- ペットを表示／隠す
- ペットを画面中央に戻す
- xangi に話しかける
- Web Chat をアプリまたはブラウザで開く
- 接続先を追加・選択・編集する
- 吹き出しやシステム通知を切り替える
- ペットや吹き出しの大きさを変える

自動徘徊や手動配置、モニター構成の変更でペットが画面外へ出た場合は、メニューバーまたは通常アプリメニューの「ペットを画面中央に戻す」を使ってください。

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
