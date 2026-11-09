# tuif への貢献ガイド

不具合報告、機能の提案、プルリクエストを歓迎します。このページでは、開発環境の準備から PR を出すまでの流れを説明します。

- 不具合報告・機能提案: [Issues](https://github.com/Az-kijitra/tuif/issues/new/choose)
- セキュリティ上の問題: Issue には書かず、[SECURITY.md](SECURITY.md) の手順で報告してください

## 開発環境

| 必要なもの | 備考 |
|---|---|
| Windows 10 / 11 | tuif は Windows 専用です（Windows API を直接使っています） |
| Rust (stable) | [rustup](https://rustup.rs/) で入れてください。`rustfmt` と `clippy` も使います |
| PowerShell 7 推奨 | インストーラーとパッケージングのスクリプトの確認に使います |

外部ツール連携（fzf / ripgrep / bat など）を確認する場合は、[ユーザーマニュアルの 1 章](docs/user_manual.md#1-前提連携ソフトウェアのインストール方法) のツールも入れてください。

## ビルドと実行

```powershell
git clone https://github.com/Az-kijitra/tuif.git
cd tuif
cargo run                # デバッグビルドで起動
cargo run -- D:\work     # 開始フォルダを指定
```

### 普段の設定を汚さずに試す

**デバッグビルドに限り**、環境変数 `TUIF_CONFIG_DIR` で設定フォルダ（既定は `%APPDATA%\tuif`）を差し替えられます。リリースビルドではこの変数は無視されます。

```powershell
$env:TUIF_CONFIG_DIR = "$env:TEMP\tuif-dev"
cargo run -- --init-config   # 設定テンプレートを作成
cargo run
```

## PR を出す前のチェック

CI（[.github/workflows/ci.yml](.github/workflows/ci.yml)）と同じチェックです。すべて通ることを確認してください。

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`install.ps1` / `uninstall.ps1` / `package.ps1` / `tools/*.ps1` を変更した場合は、CI で [PSScriptAnalyzer](https://github.com/PowerShell/PSScriptAnalyzer) のチェックも実行されます。インストーラーはユーザーの PATH と `$PROFILE` を書き換えるので、手元で動かすときは Windows サンドボックスか別のユーザーアカウントを使ってください。

## コードの構成

| パス | 役割 |
|---|---|
| `src/main.rs` | コマンドライン引数の処理、ターミナルの初期化、イベントループ |
| `src/app/` | アプリの状態 (`AppState`) と操作の処理。`keyboard.rs` / `mouse.rs`（と `mouse/`）が入力、`actions.rs` と `actions/` がアクションの実行と外部アプリの起動、`paste.rs` と `paste/`（経路別: `local.rs` / `download.rs` / `upload.rs`）がコピー・移動、`navigation.rs` / `tree.rs` / `remote.rs` が移動とツリーと SFTP |
| `src/tui/` | 画面の描画（Ratatui） |
| `src/keymap/` | キー割り当てとアクション ID |
| `src/config/` | `config.toml`・UI 状態・履歴の読み書き、設定テンプレート |
| `src/fs/`, `src/fs_ops/` | ファイル一覧の取得、コピー・移動・削除、OS クリップボード |
| `src/archive/` | ZIP の作成と展開 |
| `src/viewer/` | 内蔵ビューア |
| `src/remote/` | SFTP 接続、ホスト鍵、資格情報 |
| `src/external_apps/` | 外部ツールの起動、fzf 連携 |
| `src/theme/` | テーマと配色 |
| `tests/` | 結合テスト |
| `tools/` | 開発用の補助スクリプト（README のスクリーンショット用のダミーフォルダ作成、画像の合成）。手順は [docs/screenshots.md](docs/screenshots.md) |

主なコーディング規約は次のとおりです。全文は [.agents/skills/coding-standards/SKILL.md](.agents/skills/coding-standards/SKILL.md) にあります。

- 表示幅の計算には `unicode-width` を使ってください（日本語ファイル名の桁ずれを防ぐため）。
- 本番コードで `unwrap()` / `expect()` は使わず、エラーはステータスバーやダイアログで利用者に伝えてください。
- 描画関数（`tui/`）から、ファイル操作や設定の読み書きを直接呼ばないでください。
- 外部コマンドはシェルを介さず、`std::process::Command` に引数の配列で渡してください。

## コミットと PR

- コミットメッセージは [Conventional Commits](https://www.conventionalcommits.org/ja/) 形式にしてください（例: `fix(viewer): ...`、`feat: ...`、`docs: ...`）。
- 1 つの PR では 1 つの目的に絞ってください。
- 利用者から見える変更は、[CHANGELOG.md](CHANGELOG.md) の `[Unreleased]` に追記してください。
- PR を作ると説明欄のテンプレートが表示されるので、それに沿って書いてください。

## リリース手順（メンテナー向け）

1. `CHANGELOG.md` の `[Unreleased]` を `[x.y.z] - YYYY-MM-DD` に変え、末尾の比較リンクを更新する
2. `Cargo.toml` の `version` を更新する（`cargo build` で `Cargo.lock` も更新されます）
3. PR を作ってマージする
4. マージ後のコミットに `vx.y.z` タグを付けて push する

タグを push すると [release.yml](.github/workflows/release.yml) が動き、ZIP・`tuif.exe`・SHA256・インストーラーを添付したリリースが公開されます。リリース本文には `CHANGELOG.md` の該当バージョンの節が入ります。タグと `Cargo.toml` のバージョンが一致しないとワークフローは失敗します。

## 仕様とエージェント開発パイプライン

tuif は、仕様書を起点に AI エージェントが作業を分担する開発パイプラインで作っています。コード中のコメントに出てくる `SPEC-xxxx-xxxx` や `R-123` は、この仕様書の ID です。

| パス | 内容 |
|---|---|
| [docs/dev-pipeline.md](docs/dev-pipeline.md) | パイプラインのステージ構成 |
| [specs/_registry.md](specs/_registry.md) | 仕様 ID の台帳 |
| [specs/_issues.md](specs/_issues.md) | 仕様の矛盾や未決定事項の台帳 |
| [docs/architecture/traceability.md](docs/architecture/traceability.md) | 仕様 ⇔ 実装 ⇔ テストの対応表 |
| `.agents/skills/` | 各エージェントの役割定義と規約 |

`.agents/` のエージェント定義はこれらのパスを直接参照しているので、ファイルを移動するときは参照もあわせて直してください。

人が手で貢献する場合、このパイプラインに従う必要はありません。上の「PR を出す前のチェック」が通れば十分です。
