//! TDD (unit-test-writer, 2026-09-27): extension lists (21 total: 4 image + 2
//! markdown + 15 text), the OS-association direct list (R-414, 10 entries),
//! content sniffing (R-412) and the combined `decide_kind` order (R-410).
//!
//! Specs: SPEC-0300-0015 R-410~R-414, R-410-1, R-410-2, R-411~R-411-2,
//! R-412~R-412-3, R-414; AC-420~AC-428.
//! Architecture: SPEC-0200-0001 §3.5.1 (`decide_kind`, the sole
//! implementation, R-410-2), §3.5.4 (R-493 reuses it for previous/next).
//!
//! Contract assumed for the not-yet-implemented `decide_kind` (documented in
//! the handback report): `decide_kind(ext, sample)` performs the extension
//! comparison case-insensitively itself (R-410-1), so callers may pass the
//! extension exactly as found on disk. `sample` is read (up to 65,536 bytes,
//! R-412) and passed ONLY when steps 1~4 (R-410 順1~4) do not already decide
//! the kind; when they do, `sample` is ignored (tests below pass an empty
//! slice in those cases to make that explicit).

use super::classify::*;

const EMPTY_SAMPLE: &[u8] = &[];

// ---------------------------------------------------------------------
// extension_of (R-410-1)
// ---------------------------------------------------------------------

#[test]
fn extension_of_r410_1_is_the_text_after_the_last_dot_lowercased() {
    assert_eq!(extension_of(std::path::Path::new("a.tar.gz")).as_deref(), Some("gz"));
    assert_eq!(extension_of(std::path::Path::new("README.MD")).as_deref(), Some("md"));
    assert_eq!(extension_of(std::path::Path::new("photo.JPG")).as_deref(), Some("jpg"));
}

#[test]
fn extension_of_r410_1_a_leading_dot_only_name_has_no_extension() {
    // ".gitignore": the only dot is at index 0 -> None (R-410-1, AC-421).
    assert_eq!(extension_of(std::path::Path::new(".gitignore")), None);
}

#[test]
fn extension_of_r410_1_no_dot_at_all_has_no_extension() {
    // AC-421: "Makefile".
    assert_eq!(extension_of(std::path::Path::new("Makefile")), None);
}

#[test]
fn extension_of_r410_1_a_dotted_hidden_file_still_yields_the_last_segment() {
    // A leading dot is not special once there is a LATER dot too.
    assert_eq!(extension_of(std::path::Path::new(".env.local")).as_deref(), Some("local"));
}

// ---------------------------------------------------------------------
// content_is_text (R-412)
// ---------------------------------------------------------------------

#[test]
fn r412_step1_empty_sample_is_text() {
    // AC-424: an empty file (size 0) is text.
    assert!(content_is_text(EMPTY_SAMPLE));
}

#[test]
fn r412_step2_a_utf8_bom_is_text_even_with_a_nul_right_after() {
    // AC-424: BOM short-circuits before the NUL check (順2 before 順3).
    let sample = [0xEF, 0xBB, 0xBF, 0x00, 0x41];
    assert!(content_is_text(&sample));
}

#[test]
fn r412_step2_utf16le_and_utf16be_boms_are_text() {
    assert!(content_is_text(&[0xFF, 0xFE, 0x00, 0x41]));
    assert!(content_is_text(&[0xFE, 0xFF, 0x41, 0x00]));
}

#[test]
fn r412_step3_a_nul_byte_without_a_bom_is_binary() {
    let sample = [b'a', b'b', 0x00, b'c'];
    assert!(!content_is_text(&sample));
}

#[test]
fn r412_ac422_a_sample_that_never_saw_the_nul_stays_text() {
    // AC-422's "NUL only after byte 65,537" case: the caller never includes it
    // in the (<=65,536-byte) sample passed here, so content_is_text sees only
    // clean ASCII and must say text.
    let sample = vec![b'x'; 65_536];
    assert!(content_is_text(&sample));
}

#[test]
fn r412_1_and_2_ratio_exactly_10_percent_is_text_not_over() {
    // AC-423: exactly 10% control-byte ratio is still text ("超える" = strictly >).
    let mut sample = vec![b'a'; 90];
    sample.extend(std::iter::repeat(0x01u8).take(10)); // 0x01 is a control byte (R-412-1)
    assert_eq!(sample.len(), 100);
    assert!(content_is_text(&sample));
}

#[test]
fn r412_1_and_2_ratio_over_10_percent_is_binary() {
    let mut sample = vec![b'a'; 89];
    sample.extend(std::iter::repeat(0x01u8).take(11)); // 11% > 10%
    assert_eq!(sample.len(), 100);
    assert!(!content_is_text(&sample));
}

#[test]
fn r412_1_tab_lf_ff_cr_esc_are_excluded_from_the_control_definition() {
    // AC-423: a sample dominated by TAB/LF/FF/CR/ESC is still text, because
    // R-412-1 explicitly excludes 0x09/0x0A/0x0C/0x0D/0x1B from "control".
    let mut sample = Vec::new();
    for _ in 0..30 {
        sample.extend_from_slice(&[0x09, 0x0A, 0x0C, 0x0D, 0x1B]);
    }
    assert_eq!(sample.len(), 150);
    assert!(content_is_text(&sample));
}

#[test]
fn r412_1_the_full_control_byte_set_all_count_toward_the_ratio() {
    // Every byte in R-412-1's set pushes the ratio over 10% on its own in a
    // 5-byte sample (1/5 = 20%).
    let control_bytes: [u8; 25] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0B, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13,
        0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1C, 0x1D, 0x7F,
    ];
    for b in control_bytes {
        let sample = [b, b'a', b'a', b'a', b'a'];
        assert!(!content_is_text(&sample), "byte {b:#04x} must count as a control byte");
    }
}

// ---------------------------------------------------------------------
// decide_kind (R-410, the combined order)
// ---------------------------------------------------------------------

#[test]
fn r410_step1_image_extensions_win_regardless_of_content_case_insensitive() {
    // AC-420.
    for ext in ["png", "JPG", "jpeg", "Bmp"] {
        assert_eq!(decide_kind(Some(ext), EMPTY_SAMPLE), ViewerKind::Image, "ext={ext}");
    }
}

#[test]
fn r410_step2_markdown_extensions_case_insensitive() {
    // AC-420.
    for ext in ["md", "MARKDOWN"] {
        assert_eq!(decide_kind(Some(ext), EMPTY_SAMPLE), ViewerKind::Markdown, "ext={ext}");
    }
}

#[test]
fn r410_step3_all_15_text_extensions_case_insensitive() {
    // AC-420: the R-411 text list (15 entries).
    let text_exts = [
        "txt", "log", "json", "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx", "rs", "js", "ts",
        "py",
    ];
    assert_eq!(text_exts.len(), 15);
    for ext in text_exts {
        assert_eq!(decide_kind(Some(ext), EMPTY_SAMPLE), ViewerKind::Text, "ext={ext}");
        assert_eq!(
            decide_kind(Some(&ext.to_uppercase()), EMPTY_SAMPLE),
            ViewerKind::Text,
            "ext={} (uppercased)",
            ext.to_uppercase()
        );
    }
}

#[test]
fn r410_step4_the_10_os_open_direct_list_extensions_skip_content_sniffing() {
    // AC-426: even a sample that looks perfectly like clean text is ignored;
    // the direct list always routes to OsOpen (R-414, the confirmed #9
    // exception, ISS-0123/M-1).
    let os_open_exts = ["gif", "webp", "svg", "svgz", "ico", "tif", "tiff", "avif", "heic", "heif"];
    assert_eq!(os_open_exts.len(), 10);
    let text_like_sample = b"<svg xmlns='...'></svg>";
    for ext in os_open_exts {
        assert_eq!(decide_kind(Some(ext), text_like_sample), ViewerKind::OsOpen, "ext={ext}");
    }
}

#[test]
fn r410_step5_extension_outside_every_list_falls_back_to_content_sniffing() {
    // AC-421: `.mjs` (a real text extension not on the R-411 list) and a
    // missing extension (`Makefile`) both fall through to 順5.
    let text_sample = b"export default 1;\n";
    assert_eq!(decide_kind(Some("mjs"), text_sample), ViewerKind::Text);
    assert_eq!(decide_kind(None, text_sample), ViewerKind::Text); // e.g. `Makefile`, `.gitignore`

    let binary_sample = [0u8, 1, 2, 3];
    assert_eq!(decide_kind(Some("mjs"), &binary_sample), ViewerKind::OsOpen);
}

#[test]
fn r410_1_html_is_not_special_cased_it_falls_through_to_text_sniffing() {
    // AC-427: `.html` is not on any list; source (clean UTF-8 markup) sniffs
    // as text, so it is displayed as source, never rendered as HTML.
    let html_sample = b"<html><body>hi</body></html>";
    assert_eq!(decide_kind(Some("html"), html_sample), ViewerKind::Text);
}

#[test]
fn r410_411_1_extensions_not_on_the_15_entry_list_are_never_added_back() {
    // R-411-1: cjs/tsx/jsx/css/toml/yaml/yml/xml/ini/csv are intentionally
    // absent from the fixed lists; they are classified purely by content.
    for ext in ["cjs", "tsx", "jsx", "css", "toml", "yaml", "yml", "xml", "ini", "csv"] {
        assert_eq!(decide_kind(Some(ext), b"plain text"), ViewerKind::Text, "ext={ext}");
        assert_eq!(decide_kind(Some(ext), &[0u8, 1, 2]), ViewerKind::OsOpen, "ext={ext}");
    }
}
