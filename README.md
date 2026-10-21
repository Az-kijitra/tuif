# tuif

`tuif` は、Windows PowerShell 環境で軽快に動作する **Rust + Ratatui** 製のキーボード主体 (Vim-like) CUI/TUI ファイラーです。
GUI ファイラー「guif」の画面構成・ファイル操作・ZIP 圧縮展開・履歴・検索・内蔵テキストビューアを踏襲しつつ、外部 CUI ツール連携 (`pwsh`, `nvim`, `lazygit`, `bat`) や PowerShell のカレントディレクトリ追従 (`cd on exit`) に特化しています。

## 主な特徴

- **Vim-like キーバインド**: `h`/`j`/`k`/`l` 移動、`gg`/`G` 先頭・末尾、`y` (コピー/Yank)、`x` (切り取り/Cut)、`p` (貼り付け/Paste)、`d`/`Delete` (ごみ箱削除)、`D`/`Shift+Delete` (完全削除)、`r` (リネーム)、`a`/`A` (新規ファイル/フォルダ作成)、`/` (インクリメンタル検索: 部分一致・正規表現)。
- **外部 CUI ツール連携 (`external_apps`)**:
  - `Ctrl+Alt+C`: 対象フォルダで `pwsh.exe` (サブシェル) を開く
  - `Ctrl+Alt+V`: 対象ファイル/フォルダを `nvim.exe` で開く
  - `Ctrl+Alt+G`: 対象フォルダで `lazygit.exe` を開く
  - `Ctrl+Alt+B`: 対象ファイルを `bat.exe` (`--paging=always`) で表示
- **内蔵テキスト / Markdown ビューア**: `Enter` でテキスト・Markdown ファイルをフルスクリーン TUI ビューアで即座に閲覧 (Markdown は `leaf --inline` によるプレビュー表示と `bat` の生テキスト表示を `p`/`Tab` で切替可能、UTF-8 / UTF-16 / Shift_JIS 自動判別)。
- **ZIP 作成・展開**: `z` (圧縮) / `Z` (展開)。CP932 (Shift_JIS) エントリ名に対応。
- **PowerShell `cd on exit` 連携**: `--choosedir <FILE>` オプションにより、`tuif` 終了時のフォルダへ PowerShell のカレントディレクトリをそのまま移動可能。

## ビルドと実行

```powershell
cargo build --release
.\target\release\tuif.exe
```

## PowerShell プロファイル連携 (`cd on exit`)

`$PROFILE` に以下のラッパー関数を追加すると、`tf` コマンドで `tuif` を起動し、終了時に移動先のフォルダへ自動で `Set-Location` されます。

```powershell
function Invoke-Tuif {
    $tmp = [System.IO.Path]::GetTempFileName()
    try {
        & "C:\Users\az\git\tuif\target\release\tuif.exe" --choosedir $tmp @args
        if (Test-Path $tmp) {
            $cwd = Get-Content -Path $tmp -Raw -Encoding UTF8
            if (-not [string]::IsNullOrWhiteSpace($cwd) -and (Test-Path $cwd)) {
                Set-Location -LiteralPath $cwd.Trim()
            }
        }
    } finally {
        Remove-Item -Path $tmp -Force -ErrorAction SilentlyContinue
    }
}
Set-Alias -Name tf -Value Invoke-Tuif
```

## 詳細ドキュメント

- [開発計画書](PLAN.md)
- [ユーザーマニュアル](docs/user_manual.md)
- [仕様台帳](specs/_registry.md)
- [トレーサビリティ台帳](docs/architecture/traceability.md)
