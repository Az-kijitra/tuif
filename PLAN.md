# CUIファイラー (`cuifiler`) 開発計画書

- **作業ディレクトリ**: `C:\Users\toshi\git\cuifiler`
- **参照元リポジトリ**: `C:\Users\toshi\git\project` (GUIファイラー `ReflexFiles`)
- **作成日**: 2026-10-17

---

## 1. プロジェクト概要・目的

Windows PowerShell 環境で軽快に動作するキーボード主体の CUI/TUI ファイラーを開発します。
先行する GUI ファイラー「ReflexFiles」(`C:\Users\toshi\git\project`) の仕様・操作性・Rust バックエンド資産・開発パイプライン (Skills / Subagents) を最大限に活かしつつ、ターミナル (TUI) 環境に最適化した設計へ再構築します。

### 基本方針
1. **対象環境**: Windows 10 / 11 の PowerShell 環境 (Windows Terminal 等で実行される TUI アプリケーション)。
2. **技術スタック**: **Rust + Ratatui** (`crossterm` バックエンド)。Tauri / SvelteKit / WebView2 は使用せず、単一バイナリの純粋な Rust 製 TUI アプリとして構築する。
3. **GUI特化機能の除外とCUI連携への特化**:
   - OS関連付け起動、エクスプローラー起動、OSクリップボード(`CF_HDROP`)連携、GUI画像ビューアウィンドウ、システムトレイやインストーラー等の Windows GUI 特化機能は除外する。
   - 外部アプリ連携 (`external_apps`) は **CUIツール** (Neovim / Vim, bat, less, lazygit, git, ripgrep, fd, PowerShell サブシェル等) に限定し、TUI の一時サスペンド (Alternate Screen 解除・Raw mode 解除 → 子プロセスへ標準入出力を引き渡し → 終了後に TUI 復帰) 機構を中心に設計する。
4. **操作性の踏襲とCUI向けの見直し**:
   - ReflexFiles の画面構成 (メニューバー、パスバー、ツリー、多カラム一覧、ステータスバー) と基本機能 (ファイル操作、検索、履歴・ジャンプリスト、ZIP圧縮・展開、テキスト/Markdown内包ビューア) を踏襲する。
   - ターミナルエミュレータや PowerShell のキーバインドと競合しやすい操作、およびマウス前提の操作は CUI になじむ形へ見直す。
5. **Skill・開発資産の流用と Antigravity 対応**:
   - `C:\Users\toshi\git\project` の `.agents/skills` および `.claude/agents` で定義された Creator-Evaluator パイプラインと仕様管理ルールを Antigravity 形式 (`.agents/skills/` + `GEMINI.md` + `define_subagent` / `invoke_subagent`) に移植して活用する。

---

## 2. GUI版 (ReflexFiles) からの機能マッピング・見直し方針

### 2.1 踏襲するコア機能 (Rust バックエンドの流用対象)
`C:\Users\toshi\git\project\app\src-tauri\src` のうち、Tauri 依存層 (`commands.rs` 等) を除いた純粋ロジックは高い割合で流用可能です。

| 機能領域 | GUI版 (ReflexFiles) | CUI版 (`cuifiler`) での方針 | 流用可能な既存Rustモジュール |
|---|---|---|---|
| **画面レイアウト** | メニューバー / パスバー / ツリー + 多カラムファイル一覧 / ステータスバー | Ratatui の `Layout` で同構造を再現。ダイアログ・一覧はポップアップ (`Clear` + `Block`) で描画 | `shared/schema/ui_state.rs`, `fs/query.rs`, `fs/win_drives.rs` |
| **ファイル操作** | コピー / 切り取り / 貼り付け / リネーム / 新規ファイル・フォルダ / 削除 (ごみ箱・完全削除) | アプリ内ファイルクリップボードで実装。上書き確認・進捗・キャンセルも TUI モーダルで再現 (`trash` クレートによるごみ箱移動も維持可能) | `fs_ops/` (`create.rs`, `delete.rs`, `rename.rs`, `transfer.rs`, `preflight.rs`, `name_validation.rs`) |
| **検索** | 部分一致 + 正規表現インクリメンタルフィルタ | パスバー下の1行検索バーとしてそのまま踏襲 | フロントエンド(TS)で実装されていたフィルタロジックを Rust (`regex` クレート等) へ移植 |
| **履歴・ジャンプリスト** | 訪問履歴 (`Ctrl+Y`) / ジャンプリスト (`Ctrl+J`, `Ctrl+Shift+J`) / `navigation.toml` | ポップアップリストとしてそのまま踏襲 | `config/navigation.rs`, `shared/schema/navigation.rs` |
| **ZIP 圧縮・展開** | Deflate 圧縮・展開、CP932 対応、進捗表示、事前衝突検証 | バックグラウンドスレッド + `tokio` / `std::sync::mpsc` で非同期実行し、TUI 上でプログレスバー表示 | `archive/` 一式 (`create.rs`, `extract.rs`, `encoding_rs` 連携等) |
| **設定管理** | `config.toml`, `navigation.toml`, `ui-state.toml` | 設定ディレクトリ (`%APPDATA%\cuifiler` 等) で同じ TOML 構造を踏襲 | `config/` (`atomic.rs`, `config_toml.rs`, `paths.rs`) |

### 2.2 CUI向けに見直す機能・除外する機能

| 項目 | GUI版の仕様 | CUI版での変更・見直し案 |
|---|---|---|
| **削除の割り当て・Windows 特化機能** | ・OSの関連付けで開く (`Shift+Enter`)<br>・OSクリップボード (`CF_HDROP`) 連携<br>・別ウィンドウの ReflexViewer<br>・右クリックコンテキストメニュー<br>・Windows インストーラー (NSIS/WiX) | **除外または置換**:<br>・OS関連付け起動 / GUIアプリ起動は廃止<br>・ファイルコピー/切り取りはアプリ内バッファに限定 (※「パスのコピー」のみターミナル/OSクリップボード連携を任意検討)<br>・ビューアは別ウィンドウではなく **TUI 内の画面切り替え (フルスクリーンオーバーレイ) または外部CUIページャ (`bat` / `less`) 連携** とする<br>・右クリックメニューの代わりに **アクションパレット / コンテキストメニューキー (例: `Apps` キーまたは `m` / `.` 等)** で同等メニューを開く |
| **外部アプリ連携 (`external_apps`)** | 既定4種: Windows Terminal, VS Code, Explorer, GitHub Desktop (非同期デタッチ起動) | **CUIツール限定 (`pwsh`, `nvim`, `lazygit`, `bat`) + ターミナル制御の追加**:<br>・既定の4登録を **`pwsh` (サブシェル), `nvim` (エディタ), `lazygit` (Git管理), `bat` (ページャ/ビューア)** とする<br>・起動モード: ターミナル画面を引き渡して終了を待つ **フォアグラウンド実行** を基本とし、出力確認オプション (`pause_on_exit`) 等を設ける |
| **内蔵ビューア** | テキスト・Markdown・画像の別ウィンドウ表示、拡大縮小 | ・**テキスト / Markdown**: TUI 内蔵ビューア (`syntect` 等によるハイライト、またはプレーン表示 + Markdown 簡易レンダリング) を単一画面切り替えで提供<br>・**画像表示**: スコープ外 (対象外) とする |
| **キーバインドの見直し** | Arrowキー移動、`Ctrl+C` (コピー), `Ctrl+V` (貼り付け), `Ctrl+X` (切り取り) 等 | **Vim like キーバインドの採用**:<br>・移動: `h` / `j` / `k` / `l` (および矢印キー併用)、`gg` / `G` (先頭/末尾)、`Ctrl+u` / `Ctrl+d` または `Ctrl+b` / `Ctrl+f` (ページ移動)<br>・ファイル操作: `y` (コピー / yank)、`d` または `x` (切り取り / cut)、`p` (貼り付け / paste)、`dd` / `Delete` (ごみ箱削除)、`D` / `Shift+Delete` (完全削除)、`r` / `F2` (リネーム)、`a` (新規ファイル)、`A` (新規フォルダ)<br>・選択・検索・その他: `Space` / `v` (選択トグル)、`V` / `Ctrl+A` (全選択)、`/` (検索)、`q` / `Ctrl+Q` (終了)<br>・PowerShell のディレクトリ連携機能 (**終了時にカレントディレクトリを PowerShell 側へ引き継ぐ `cd on exit` ラッパー関数**) を提供する。 |
| **テーマ設定** | `"light"` / `"dark"` / `"system"` (WebView CSS) | ANSI 16色 / 256色 / TrueColor に対応し、ターミナルの背景透過・配色に馴染む `"dark"` / `"light"` / `"ansi"` テーマへ見直す。 |

---

## 3. アーキテクチャ設計案 (Rust + Ratatui)

Tauri の「フロントエンド (SvelteKit) ⇔ IPC ⇔ バックエンド (Rust)」の2層構造を、単一 Rust クレート内のレイヤードアーキテクチャに統合します。

```text
cuifiler/
├── Cargo.toml
├── GEMINI.md                     # Antigravity 用プロジェクト全体ルール・パイプライン定義
├── .agents/
│   └── skills/                   # project から移植・最適化した Antigravity Skills
│       ├── coding-standards/
│       ├── pr-conventions/
│       ├── spec-management/
│       └── dev-pipeline/         # 9ステージ Creator-Evaluator 定義・プロンプト集
├── docs/
│   ├── dev-pipeline.md
│   ├── user_manual.md
│   └── architecture/
│       ├── architecture.md
│       └── traceability.md
├── specs/                        # 仕様書群 (SPEC-xxxx-xxxx)
├── tests/                        # テストケース台帳・結果台帳
└── src/
    ├── main.rs                   # エントリポイント、ターミナル初期化・復元・パニックフック
    ├── app.rs                    # アプリ全体の状態 (AppState) とメインイベントループ
    ├── tui/                      # Ratatui 描画・Crossterm イベント処理・サブプロセス画面復帰
    │   ├── terminal.rs           # AlternateScreen / RawMode 管理、外部CUIツール実行時の suspend/resume
    │   ├── event.rs              # キー入力・リサイズ・非同期タスク通知の集約ループ
    │   ├── theme.rs              # TUI カラーパレット定義
    │   ├── layout.rs             # メニューバー / パスバー / ツリー / 一覧 / ステータスバーの分割
    │   └── widgets/              # 各画面コンポーネント (file_list, tree, menu_bar, modal, viewer 等)
    ├── keymap/                   # Vim-like キー正規化・ディスパッチ優先順位・アクション解決
    ├── domain/                   # 旧 project の src-tauri/src から流用するコアロジック
    │   ├── fs/                   # ディレクトリ列挙・ドライブ列挙・ソート・検索フィルタ
    │   ├── fs_ops/               # コピー・移動・削除・リネーム・新規作成・衝突検出
    │   ├── archive/              # ZIP 圧縮・展開
    │   ├── viewer/               # テキスト/バイナリ判定、テキスト読み込み
    │   └── external_apps/        # CUI外部ツール解決 (`pwsh`, `nvim`, `lazygit`, `bat`)・引数展開・プロセス起動
    ├── config/                   # config.toml, navigation.toml, ui-state.toml の読み書き
    └── shared/                   # 絶対パス検証 (`abs_path`), キャンセル制御, 共通スキーマ
```

### 技術選定 (予定クレート)
- **TUI / 端末制御**: `ratatui`, `crossterm`
- **シリアライズ / 設定**: `serde`, `toml`
- **ファイル操作 / ごみ箱 / ZIP / 文字コード**: `trash`, `zip`, `time`, `encoding_rs`, `windows-sys` (ReflexFiles と同一構成)
- **検索 / 正規表現 / 幅計算**: `regex`, `unicode-width` (日本語ファイル名の全角カラム幅計算に必須)
- **テスト**: 標準 `#[cfg(test)]` + `ratatui::backend::TestBackend` (ヘッドレスでの画面描画スナップショット・状態遷移検証が可能。GUI版のWebView E2Eよりも高速かつ決定論的に全ACを自動テスト化できる)

---

## 4. `C:\Users\toshi\git\project` の Skill 流用と Antigravity 対応計画

### 4.1 現状の分析 (`C:\Users\toshi\git\project`)
参照元プロジェクトには以下の AI エージェント資産が存在します。
1. **Skills (`.agents/skills/` および `.claude/skills/`)**:
   - `spec-management/SKILL.md`: Spec ID 採番・階層化・ステータス遷移 (`DRAFT` → `IN_REVIEW` → `APPROVED`)・Issue管理ルール
   - `coding-standards/SKILL.md`: コーディング規約 (現状はプレースホルダー)
   - `pr-conventions/SKILL.md`: コミットメッセージ・PR規約
2. **Subagents (`.claude/agents/*.md`, `.codex/agents/*.toml`)**:
   - 9ステージの Creator / Evaluator ペア計19種のエージェント定義 (`requirements-extractor`, `spec-writer`, `spec-reviewer`, `architecture-mapper`, `architecture-guardian`, `unit-test-writer`, `code-generator`, `code-reviewer`, `testcase-writer`, `coverage-auditor`, `test-automation-engineer`, `test-debugger`, `test-result-recorder`, `commit-pr-manager`, `pr-reviewer`, `dev-pipeline-orchestrator` 等)

### 4.2 Antigravity 向けの変更・移行内容
Antigravity のカスタマイズ仕様 (`agy-customizations`) に合わせ、以下の通り変換して `C:\Users\toshi\git\cuifiler` に配置します。

1. **Workspace Skills の配置 (`.agents/skills/`)**:
   - Antigravity はワークスペース直下の `.agents/skills/<name>/SKILL.md` を自動認識します。
   - **変更点1 (`coding-standards/SKILL.md`)**: プレースホルダー状態から、**Rust + Ratatui 固有の規約** (`cargo fmt`, `cargo clippy -- -D warnings`, `unwrap()` 禁止とエラー型設計、モジュール行数基準、TUI 描画関数と状態変更ロジックの分離、`unicode-width` による全角幅扱いの徹底) に具体化します。
   - **変更点2 (`spec-management/SKILL.md` / `pr-conventions/SKILL.md`)**: そのまま流用しつつ、Tauri/Svelte 前提の表現があれば Rust/Ratatui 構成に合わせて調整します。
   - **変更点3 (`dev-pipeline/SKILL.md` の新設)**: `.claude/agents/*.md` にあった19個のエージェント定義は Claude Code 独自のフロントマター (`tools:`, `model: opus/sonnet`) を持っています。Antigravity では `define_subagent` / `invoke_subagent` ツールを使って動的にサブエージェントを定義・起動するため、`.claude/agents/` の各プロンプトを `.agents/skills/dev-pipeline/references/agents/` 配下に整理し、`dev-pipeline/SKILL.md` から Antigravity の `define_subagent` で呼び出す手順書としてパッケージングします。
2. **Workspace Rules の配置 (`GEMINI.md` / `AGENTS.md`)**:
   - リポジトリルートに `GEMINI.md` を配置し、`docs/dev-pipeline.md` の原則 (Creator-Evaluator の分離、`APPROVED` 遷移や `PENDING_HUMAN_REVIEW` における人間承認ゲートの厳守) を常時有効ルールとして定義します。

---

## 5. 開発フェーズ計画 (ステップ・バイ・ステップ)

開発は以下のフェーズに分け、各フェーズで「仕様策定 (Creator/Evaluator + 人間承認) → 設計 → 実装・単体テスト → `TestBackend` による統合テスト」のサイクルを回します。

### Phase 0: ワークスペース構築・Skill 移植・初期仕様定義
1. `C:\Users\toshi\git\cuifiler` に Git リポジトリ・ディレクトリ構造 (`specs/`, `docs/`, `tests/`, `.agents/skills/`) を初期化。
2. `C:\Users\toshi\git\project` から Skills と Subagent 定義を Antigravity 形式に変換して移植 (`GEMINI.md` 作成含む)。
3. プロダクト方針仕様 (`SPEC-0100-*`) と技術スタック・アーキテクチャ仕様 (`SPEC-0200-0001`: Rust + Ratatui 構成、GUI版からの変更点一覧) を起票・レビュー・承認。
4. Cargo プロジェクト (`Cargo.toml`) の作成と、GUI版からの共通バックエンドモジュール (`shared/`, `config/`, `fs/`, `fs_ops/`, `archive/`) の移植・単体テスト通過確認。

### Phase 1: TUI 基盤・ファイル一覧・ツリー・Vim-like キーボードナビゲーション (MVP土台)
1. **対象仕様**: ファイル一覧/ツリー表示 (`SPEC-0300-0009` 相当) + Vim-like キーボードナビゲーション (`SPEC-0300-0010` 相当) + パスバー/ドライブ切替 (`SPEC-0300-0016` 相当) + カラースキーム (`SPEC-0300-0017` 相当) の CUI 版仕様確定。
   - **PowerShell 連携 (`--choosedir` / 終了時ディレクトリ出力オプション)** も合わせて確定する。
2. **実装**:
   - `crossterm` + `ratatui` による画面描画ループ。
   - 多カラムファイル一覧・日本語全角文字幅対応・ソート・隠しファイル切替・ドライブツリー表示。
   - `config.toml` (`input_keymap_custom`, `ui_theme`) と `ui-state.toml` の読み書き。

### Phase 2: ファイル操作 (コピー / 切り取り / 貼り付け / 削除 / リネーム / 新規作成)
1. **対象仕様**: ファイル操作詳細仕様 (`SPEC-0300-0011` 相当、OSクリップボード連携を除きアプリ内バッファ + Vim-like キー操作 `y`/`x`/`p`/`r`/`a`/`A` に最適化)。
2. **実装**:
   - コピー/切り取りバッファ、衝突検出ダイアログ (上書き・マージ・スキップ・リネーム)、進捗モーダルとキャンセル (`Esc`)、ごみ箱削除 / 完全削除。

### Phase 3: 検索・訪問履歴・ジャンプリスト
1. **対象仕様**: インクリメンタル検索 (`SPEC-0300-0012` 相当) + 履歴・ジャンプリスト (`SPEC-0300-0013` 相当)。
2. **実装**:
   - `/` キーによる検索バー表示・部分一致/正規表現フィルタリング。
   - 訪問履歴 / ジャンプリスト ポップアップと `navigation.toml` 永続化。

### Phase 4: ZIP 圧縮・展開 & TUI 内蔵ビューア
1. **対象仕様**: ZIP 作成・展開 (`SPEC-0300-0014` 相当) + 内蔵テキスト/Markdownビューア (`SPEC-0300-0015` 相当のCUI版、画像はスコープ外)。
2. **実装**:
   - ZIP 非同期圧縮・展開と TUI プログレス表示。
   - TUI フルスクリーンオーバーレイによるテキスト/Markdown ビューア (行番号、`j`/`k`/`Ctrl+d`/`Ctrl+u` スクロール、前後ファイル移動、シンタックスハイライト)。

### Phase 5: メニューバー・アクションメニュー・外部 CUI アプリ連携
1. **対象仕様**: メニューバー (`SPEC-0300-0020` 相当) + コンテキストアクションメニュー (`SPEC-0300-0018` 相当のキーボード起動版) + 外部CUIアプリ起動 (`SPEC-0300-0021` 相当のCUIツール限定版)。
2. **実装**:
   - メニューバー (`Alt+F/E/V/G` または `F10`) とアクションメニューの TUI ポップアップ実装。
   - 外部 CUI アプリ起動 (既定4種: `pwsh`, `nvim`, `lazygit`, `bat`): TUI の Alternate Screen 一時解除 → 子プロセス実行と待機 → 画面再描画の実装。

### Phase 6: ドキュメント整備・PowerShell 統合スクリプト・リリースビルド
1. README、ユーザーマニュアル (`docs/user_manual.md`)、PowerShell プロファイル用ラッパー関数 (`Invoke-CuiFiler` 等) の整備。
2. 全受入基準 (AC) のテスト台帳 (`tests/results/ledger.md`) 記録とリリースバイナリ (`cargo build --release`) の検証。

---

## 6. 確定した基本方針 (2026-10-17 合意済み)

1. **アプリ名称 (バイナリ名・設定フォルダ名)**:
   - アプリ名・バイナリ名ともに **`cuifiler`** とする (設定フォルダは `%APPDATA%\cuifiler`)。
2. **キーバインド方針**:
   - **Vim like** とする (`h`/`j`/`k`/`l` 移動、`y` コピー、`x`/`d` 切り取り、`p` 貼り付け、`r` リネーム、`a`/`A` 新規作成、`/` 検索など。`config.toml` の `[input_keymap_custom]` によるカスタマイズも維持)。
3. **外部 CUI ツールの既定4枠 (`external_apps`)**:
   - 既定の4登録は **`pwsh`**, **`nvim`**, **`lazygit`**, **`bat`** とする。
     1. **PowerShell (`pwsh.exe`)**: 対象フォルダ (`{dir}`) でサブシェルを起動
     2. **Neovim (`nvim.exe`)**: 対象ファイルまたはフォルダ (`{path}`) を開く
     3. **Lazygit (`lazygit.exe`)**: 対象フォルダ (`{dir}`) で起動 (Git管理有無を問わない)
     4. **bat (`bat.exe`)**: 対象ファイル (`{path}`) をページング表示 (`--paging=always`)
4. **内蔵ビューアのスコープ**:
   - テキスト / Markdown は TUI 内蔵ビューアで表示し、**画像はスコープ外** とする。
