# Changelog

このプロジェクトの主な変更点を記録します。
書式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/) に、バージョン番号は [Semantic Versioning](https://semver.org/lang/ja/) に従います。

## [Unreleased]

### 修正
- ローカルフォルダで「新規フォルダ作成」(`A` / `Ctrl+Shift+N` / メニュー) が常に `InvalidKind` で失敗していた問題を修正しました。
- SFTP 接続プロファイルの編集画面で、`2222` など同じ数字が続くポート番号を入力できなかった問題を修正しました。新規プロファイルのポート欄は空欄で始まり、空欄のまま保存すると `22` になります。

## [0.1.1] - 2026-11-08

### 追加
- ファイル一覧とツリーにアイコンを表示します。`config.toml` の `[layout]` にある `icons` で `"nerd"`（Nerd Font、既定）/ `"unicode"`（絵文字）/ `"ascii"` / `"none"` を選べます。
- `tuif --init-config`: `config.toml` が無いときにテンプレートを作成します。エディタは開かず、既存のファイルは変更しません。

### 修正
- `config.toml` の `[[external_apps]]` が読み込まれていなかった問題を修正しました。組み込みツールと同じ名前なら置き換え、それ以外は追加します。設定に誤りがあれば起動時にステータスバーへ表示します。
- 設定テンプレートに、実際には読み込まれない項目（`[layout]` の `show_tree` / `show_hidden` / `show_size` / `show_time`）が含まれていた問題を修正しました。
- インストーラー (`install.ps1`)
  - `irm ... | iex` でインストールしたときに `uninstall.ps1` が配置されなかった問題を修正しました。
  - ユーザー PATH を更新すると、`%USERPROFILE%` などの環境変数を含む既存のエントリが展開済みの固定パスに置き換わる問題を修正しました（アンインストール時も同様）。
  - `irm ... | iex` で実行したあと、変数や `$ErrorActionPreference` の変更が呼び出し元のシェルに残る問題を修正しました。
- アンインストーラー (`uninstall.ps1`) はインストール先フォルダを丸ごと削除せず、tuif が作成したファイルだけを削除するようになりました。
- ユーザーマニュアルで `j` / `k` に対応する矢印キーが逆に書かれていた誤りを修正しました。

### セキュリティ
- インストーラーはダウンロードした `tuif.exe` を、リリースに添付した `tuif.exe.sha256` で検証するようになりました。

### その他
- リリースに `tuif.exe.sha256`・`install.ps1`・`uninstall.ps1` を添付するようにしました。
- CI に release ビルドと PowerShell スクリプトの静的解析 (PSScriptAnalyzer) を追加しました。

## [0.1.0] - 2026-11-05

最初の公開リリースです。

- Vim 風キー操作とマウス操作に対応した、ツリーとマルチカラム一覧の TUI ファイラー
- Windows エクスプローラーとのクリップボード連携 (`CF_HDROP`)、ごみ箱への削除
- 内蔵テキスト / Markdown ビューア（UTF-8 / UTF-16 / Shift_JIS 自動判別）
- ZIP の作成と展開（CP932 エントリ名対応）
- SFTP 接続（TOFU によるホスト鍵確認、Windows 資格情報マネージャーへのパスワード保存）
- fzf / ripgrep / bat / Neovim / Lazygit / VS Code / pwsh との連携
- PowerShell の cd on exit (`tf`) と、ワンライナーで導入できるインストーラー
- 6 種類のテーマ、色の個別上書き、キーマップのカスタマイズ

[Unreleased]: https://github.com/Az-kijitra/tuif/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/Az-kijitra/tuif/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Az-kijitra/tuif/releases/tag/v0.1.0
