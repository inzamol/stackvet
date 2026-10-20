use stackvet_core::{
    byte_offset_to_line_col, byte_range_to_span, extract_snippet_from_source, Diagnostic,
    DiagnosticLocation, FixAction, LanguageTarget, RuleId, Severity, TextSpan,
};
use std::path::PathBuf;

#[test]
fn test_text_span_methods() {
    let single = TextSpan::single_line(10, 5, 20);
    assert!(!single.is_multiline());
    assert_eq!(single.line_count(), 1);
    assert!(single.contains_line(10));
    assert!(!single.contains_line(9));
    assert!(!single.contains_line(11));

    let multi = TextSpan::new(10, 15, 1, 10);
    assert!(multi.is_multiline());
    assert_eq!(multi.line_count(), 6);
    assert!(multi.contains_line(10));
    assert!(multi.contains_line(12));
    assert!(multi.contains_line(15));
    assert!(!multi.contains_line(16));
}

#[test]
fn test_unicode_character_offsets() {
    // Emojis and CJK characters have multi-byte UTF-8 representations
    // "🚀 hello 日本語"
    // '🚀' = 4 bytes, 1 char
    // ' '  = 1 byte, 1 char
    // 'h'  = 1 byte, 1 char (offset 5 bytes, col 3)
    let source = "🚀 hello 日本語\nsecond line\n";

    // Start of line 1: offset 0 -> line 1, col 1
    assert_eq!(byte_offset_to_line_col(source, 0), (1, 1));

    // After rocket (4 bytes) -> ' ' at byte 4 -> line 1, col 2
    assert_eq!(byte_offset_to_line_col(source, 4), (1, 2));

    // 'h' in "hello" is at byte 5 -> line 1, col 3
    assert_eq!(byte_offset_to_line_col(source, 5), (1, 3));

    // "second line" is on line 2
    let line2_offset = source.find("second").unwrap();
    assert_eq!(byte_offset_to_line_col(source, line2_offset), (2, 1));
}

#[test]
fn test_byte_range_to_span() {
    let source = "def greet():\n    # 🌟 sparkles\n    return 'hello'\n";
    let ret_start = source.find("return").unwrap();
    let ret_end = ret_start + "return 'hello'".len();

    let span = byte_range_to_span(source, ret_start, ret_end);
    assert_eq!(span.start_line, 3);
    assert_eq!(span.start_col, 5); // 4 spaces indentation + 1
    assert_eq!(span.end_line, 3);
    assert_eq!(span.end_col, 5 + "return 'hello'".len());
}

#[test]
fn test_extract_snippet_from_source() {
    let source = "line 1\nline 2 with target\nline 3\nline 4\n";

    // Single line snippet
    let span_single = TextSpan::single_line(2, 8, 12);
    let snippet_single = extract_snippet_from_source(source, &span_single);
    assert_eq!(snippet_single.as_deref(), Some("with"));

    // Multiline snippet
    let span_multi = TextSpan::new(2, 3, 1, 1);
    let snippet_multi = extract_snippet_from_source(source, &span_multi);
    assert_eq!(snippet_multi.as_deref(), Some("line 2 with target\nline 3"));

    // Invalid span rejection (zero lines or zero columns)
    assert_eq!(extract_snippet_from_source(source, &TextSpan::new(0, 2, 1, 5)), None);
    assert_eq!(extract_snippet_from_source(source, &TextSpan::new(2, 2, 0, 5)), None);
    assert_eq!(extract_snippet_from_source(source, &TextSpan::new(2, 2, 1, 0)), None);
}

#[test]
fn test_path_normalization() {
    // Windows UNC path normalization
    let loc_unc = DiagnosticLocation::new(r"\\?\C:\Projects\app\main.py");
    assert_eq!(
        loc_unc.normalized_path().as_deref(),
        Some("C:/Projects/app/main.py")
    );

    // Standard Windows backslashes
    let loc_win = DiagnosticLocation::new(r"src\components\Button.tsx");
    assert_eq!(
        loc_win.normalized_path().as_deref(),
        Some("src/components/Button.tsx")
    );

    // POSIX path preserved
    let loc_posix = DiagnosticLocation::new("src/index.js");
    assert_eq!(loc_posix.normalized_path().as_deref(), Some("src/index.js"));

    // Synthetic / unknown path represented as None
    let loc_unknown = DiagnosticLocation::new("<unknown>");
    assert_eq!(loc_unknown.normalized_path(), None);
    assert!(loc_unknown.is_empty());

    let loc_empty = DiagnosticLocation::new("");
    assert_eq!(loc_empty.normalized_path(), None);
    assert!(loc_empty.is_empty());
}

#[test]
fn test_consistent_display_location() {
    // Single line with column
    let loc1 = DiagnosticLocation::new(r"src\main.py").with_span(TextSpan::new(10, 10, 5, 12));
    assert_eq!(loc1.display_location(), "src/main.py:10:5");

    // Single line col 1
    let loc2 = DiagnosticLocation::new("src/main.py").with_span(TextSpan::single_line(10, 1, 1));
    assert_eq!(loc2.display_location(), "src/main.py:10");

    // Multiline span
    let loc3 = DiagnosticLocation::new(r"src\lib.rs").with_span(TextSpan::new(10, 12, 1, 1));
    assert_eq!(loc3.display_location(), "src/lib.rs:10-12");

    // File only, no span
    let loc4 = DiagnosticLocation::new("Dockerfile");
    assert_eq!(loc4.display_location(), "Dockerfile");

    // Missing / synthetic location displays explicitly as "-"
    let loc5 = DiagnosticLocation::new("<unknown>");
    assert_eq!(loc5.display_location(), "-");
}

#[test]
fn test_diagnostic_builder_with_related_locations() {
    let related1 = DiagnosticLocation::new("src/first.py")
        .with_span(TextSpan::single_line(5, 1, 10))
        .with_message("First declaration");
    let related2 = DiagnosticLocation::new("src/second.py")
        .with_span(TextSpan::single_line(20, 1, 10))
        .with_message("Conflicting declaration");

    let diag = Diagnostic::builder("IMP-001", Severity::Error, LanguageTarget::Python)
        .message("Cycle detected")
        .file(PathBuf::from("src/cycle.py"))
        .related_location(related1.clone())
        .related_location(related2.clone())
        .build();

    assert_eq!(diag.related_locations.len(), 2);
    assert_eq!(
        diag.related_locations[0].display_location(),
        "src/first.py:5"
    );
    assert_eq!(
        diag.related_locations[0].message.as_deref(),
        Some("First declaration")
    );
    assert_eq!(
        diag.related_locations[1].display_location(),
        "src/second.py:20"
    );
}
