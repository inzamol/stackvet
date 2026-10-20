use stackvet_core::DiagnosticLocation;
use stackvet_node::parse_node_imports;
use std::fs;
use std::path::Path;

#[test]
fn test_node_golden_fixture_locations() {
    let fixture_path = Path::new("../../tests/fixtures/golden_locations/unicode_and_multiline.ts");
    let content = fs::read_to_string(fixture_path)
        .or_else(|_| fs::read_to_string("tests/fixtures/golden_locations/unicode_and_multiline.ts"))
        .expect("read ts golden fixture");

    let imports = parse_node_imports(&content, fixture_path);

    // 1. Top-level import fs from 'fs' (after Unicode comments)
    let fs_imp = imports
        .iter()
        .find(|i| i.specifier == "fs")
        .expect("import fs found");
    assert_eq!(fs_imp.line, 4);
    let fs_span = fs_imp.span.expect("fs span");
    assert_eq!(fs_span.start_line, 4);
    assert_eq!(fs_span.end_line, 4);

    // 2. Nested multiline dynamic import inside runTask()
    let util_imp = imports
        .iter()
        .find(|i| i.specifier == "./utils")
        .expect("import ./utils");
    assert!(util_imp.is_dynamic);
    assert_eq!(util_imp.line, 8);
    let util_span = util_imp.span.expect("utils span");
    assert_eq!(util_span.start_line, 8);
    assert_eq!(util_span.end_line, 10);
    assert!(util_span.is_multiline());

    // 3. Multiline export from './constants'
    let const_imp = imports
        .iter()
        .find(|i| i.specifier == "./constants")
        .expect("import ./constants");
    assert_eq!(const_imp.line, 13);
    let const_span = const_imp.span.expect("constants span");
    assert_eq!(const_span.start_line, 13);
    assert_eq!(const_span.end_line, 16);
    assert!(const_span.is_multiline());

    // Snippet extraction
    let snippet = DiagnosticLocation::extract_snippet_from_source(&content, &const_span);
    assert!(snippet.is_some());
    assert!(snippet.unwrap().contains("./constants"));
}
