# インストール手順

xangi-pets の配布・実機確認対象はmacOS Apple Siliconです。Developer ID署名はなく、初回起動時にセキュリティ警告が出る場合があります。

Windows x86_64とLinux x86_64はGitHub Actionsでパッケージのビルドまで確認しているが、実機動作は未確認のためGitHub Releaseでは配布していない。

## macOS（Apple Silicon）

配布アプリはad-hoc署名済みです。

### 推奨: 右クリックで開く

1. `xangi-pets_X.Y.Z_aarch64.dmg` をダブルクリックしてマウント
2. `xangi-pets.app` を `/Applications` にドラッグ
3. **Finder で `xangi-pets.app` を右クリック → 「開く」**（普通のダブルクリックではなく、右クリックメニューから）
4. 警告ダイアログで「開く」を選択

一度通せば 2 回目以降は普通にダブルクリックで起動できる。

起動後は接続先を追加します。表示・接続・通知はメニューバーから操作でき、アイコンが見えない場合はDockでアプリを選んで通常アプリメニューを使えます。詳しくは[使い方ガイド](usage.md)を参照してください。

### もし「壊れているため開けません」と出たら

GitHub からダウンロードしたファイルには `com.apple.quarantine` 拡張属性が付いていて、その状態で署名検証に失敗するとこの警告になる。ターミナルで:

```bash
xattr -cr /Applications/xangi-pets.app
```

実行後にもう一度起動。

## Windows / Linux

現在は配布対象外。ソースからの開発・検証は可能だが、実機確認が完了するまではインストーラをGitHub Releaseへ掲載しない。

## アップデート

アプリを終了してから、新しいバージョンを同じ手順でコピーします。接続先・サイズ・通知・位置の設定は保持されます。

接続プロファイルはアプリ設定ディレクトリの`connection-profiles.json`、全ペット共通サイズは`pet-display.json`、表示するセッションの種類は`session-filters.json`に保存します。吹き出し・通知・常駐ペットの位置はWebViewのlocalStorageに接続プロファイル別で保存します。

## アンインストール

アプリを終了し、`/Applications/xangi-pets.app` をFinderでゴミ箱へ移動します。設定データは残ります。
