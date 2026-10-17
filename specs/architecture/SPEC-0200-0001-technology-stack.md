# SPEC-0200-0001: 技術スタックとレイヤードアーキテクチャ (Rust + Ratatui)

| 項目 | 内容 |
|---|---|
| Spec ID | SPEC-0200-0001 |
| 領域 | 0200 アーキテクチャ・技術基盤 |
| 階層 | 機能レベル (親: SPEC-0100-0001) |
| ステータス | APPROVED (2026-10-17 発注者承認) |
| 作成日 | 2026-10-17 |
| 一次出典 | 発注者要求・基本方針合意 (2026-10-17, `PLAN.md` §3, `ISS-0001`) |

## 1. 技術スタック

- **[R-201] 言語・TUI フレームワーク**: Rust (2021 edition) + `ratatui` + `crossterm` を採用し、単一実行可能バイナリ (`cuifiler.exe`) として構成する。
- **[R-202] コア依存クレート**:
  - TUI・端末制御: `ratatui`, `crossterm`, `unicode-width`
  - 設定・シリアライズ: `serde`, `toml`
  - ファイル操作・ZIP・文字コード: `trash`, `zip` (`deflate`, `time`), `time`, `encoding_rs`, `windows-sys` (`Win32_Foundation`, `Win32_Storage_FileSystem`)
  - 検索・正規表現: `regex`
  - テスト: `tempfile`, `ratatui::backend::TestBackend`

## 2. レイヤードアーキテクチャと依存方向

- **[R-203] モジュール構成**:
  - `shared/`: 絶対パス検証 (`abs_path`)、キャンセル制御 (`cancel`)、同一性判定 (`identity`)、共通設定スキーマ (`schema/`)。他のいかなるモジュールにも依存しない最下層。
  - `config/`: `config.toml`、`navigation.toml`、`ui-state.toml` の原子的読み書き (`atomic.rs`) とパス解決 (`paths.rs`)。`shared/` のみに依存する。
  - `domain/`: ファイルシステム列挙 (`fs/`)、ファイル操作 (`fs_ops/`)、ZIP 圧縮・展開 (`archive/`)、ビューア判定・読込 (`viewer/`)、外部CUIツール起動 (`external_apps/`)。`shared/` のみに依存し、TUI (`ratatui`/`crossterm`) には依存しない。
  - `keymap/`: Vim-like キー入力正規化、マルチキーシーケンス (`gg`, `dd` 等)、アクション解決。`shared/` のみに依存する。
  - `tui/`: Ratatui ウィジェット描画、レイアウト計算、テーマ配色、ターミナル制御（Alternate Screen の一時サスペンドと復帰）。描画関数は状態を変更しない純粋なビューとして実装する。
  - `app`: アプリケーション全体の状態 (`AppState`) とアクションディスパッチを統合する。
- **[R-204] モジュール分割基準**:
  - 1ファイルあたり400行以下を目安とし、肥大化を防ぐ。
  - 単体テストは対象モジュールの隣 (`*_tests.rs`) または `#[cfg(test)]` モジュールに配置し、`TestBackend` によるヘッドレス TUI テストで画面描画とキー操作を一貫検証する。
