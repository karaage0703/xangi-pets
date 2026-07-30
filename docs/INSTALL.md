# インストール手順

xangi-pets は **個人開発の無料アプリ**で、現在の配布・実機確認対象はmacOS Apple Siliconのみ。Apple Developer IDによる正式なコード署名は付いていないため、初回起動時のセキュリティ警告を通す手順をまとめる。

Windows x86_64とLinux x86_64はGitHub Actionsでパッケージのビルドまで確認しているが、実機動作は未確認のためGitHub Releaseでは配布していない。

## macOS（Apple Silicon）

ad-hoc 署名済みなので **「壊れているため開けません」** は出ない想定だが、Gatekeeper が **「開発元未確認」** と表示する。

### 推奨: 右クリックで開く

1. `xangi-pets_X.Y.Z_aarch64.dmg` をダブルクリックしてマウント
2. `xangi-pets.app` を `/Applications` にドラッグ
3. **Finder で `xangi-pets.app` を右クリック → 「開く」**（普通のダブルクリックではなく、右クリックメニューから）
4. 警告ダイアログで「開く」を選択

一度通せば 2 回目以降は普通にダブルクリックで起動できる。

起動後はmacOSのメニューバーにxangi-petsアイコンが常駐する。ペットを隠した場合も、メニューバーから再表示、xangiへの入力、アプリ内Web Chat、接続先設定へアクセスできる。

メニューバーがほかの常駐アイコンで埋まりxangi-petsが見えない場合は、Dockでxangi-petsを選択し、画面左上の通常アプリメニューを使う。主要操作、接続状態、内蔵サーバのポート、通知設定は両方のメニューに表示される。Web Chatはxangi-pets内の通常ウィンドウまたは既定ブラウザで開ける。

通知は初回起動では無効で、macOSの通知許可も要求しない。メニューバーの「通知」をONにした後、最初の通知を表示するときにmacOSから許可を求められる場合がある。

### もし「壊れているため開けません」と出たら

GitHub からダウンロードしたファイルには `com.apple.quarantine` 拡張属性が付いていて、その状態で署名検証に失敗するとこの警告になる。ターミナルで:

```bash
xattr -cr /Applications/xangi-pets.app
```

実行後にもう一度起動。

## Windows / Linux

現在は配布対象外。ソースからの開発・検証は可能だが、実機確認が完了するまではインストーラをGitHub Releaseへ掲載しない。

## アップデート

新しいバージョンが出たら同じ手順で再インストール。設定（xangi URL、選択中ペット、ペット/バブルサイズ等）はアプリのlocalStorageに残るので、引き続き使える。

## アンインストール

- **macOS**: `/Applications/xangi-pets.app` を Finder でゴミ箱へ。設定を完全削除したい場合は `~/.xangi/` も削除
