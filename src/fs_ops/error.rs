//! SPEC-0300-0011 §10 (R-88): the Undo/Redo scope-exclusion guard.
//! Error-code translation is owned by the frontend table only
//! (`app/src/lib/shared/messages.ts`, SPEC-0300-0019 T-13); there is no
//! `translate_error_code` here.

#[cfg(test)]
mod tests {
    // SPEC-0300-0011 AC-74 (R-88): the source tree must contain no
    // Undo/Redo-scoped error code or translation strings (`undo_unavailable`
    // / `redo_unavailable`). A standing regression guard
    // (SPEC-0100-0005 R-08: no trace of scope-excluded features).
    #[test]
    fn ac74_source_tree_has_no_undo_redo_error_strings() {
        // 自己言及回避: この検査自身のファイル(禁止文字列リテラルを保持する
        // ソースファイル)はスキャン対象から除外する。除外しない場合、この
        // テストは実装の正誤に関わらず常に自分自身を検出して失敗する
        // (自己言及的なテスト設計上のバグ。code-generatorが2026-09-23に検出)。
        let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let self_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file!());
        let forbidden = ["undo_unavailable", "redo_unavailable"];
        let mut offending_files = Vec::new();

        fn walk(
            dir: &std::path::Path,
            forbidden: &[&str],
            self_path: &std::path::Path,
            offending: &mut Vec<std::path::PathBuf>,
        ) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, forbidden, self_path, offending);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    if path
                        .canonicalize()
                        .ok()
                        .zip(self_path.canonicalize().ok())
                        .is_some_and(|(a, b)| a == b)
                    {
                        continue;
                    }
                    if let Ok(contents) = std::fs::read_to_string(&path) {
                        if forbidden.iter().any(|needle| contents.contains(needle)) {
                            offending.push(path);
                        }
                    }
                }
            }
        }

        walk(&src_root, &forbidden, &self_path, &mut offending_files);
        assert!(
            offending_files.is_empty(),
            "found forbidden undo/redo error strings in: {offending_files:?}"
        );
    }
}
