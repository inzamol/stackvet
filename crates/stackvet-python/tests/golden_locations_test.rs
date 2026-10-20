use stackvet_core::DiagnosticLocation;
use stackvet_python::parse_python_imports;
use std::fs;
use std::path::Path;

#[test]
fn test_python_golden_fixture_locations() {
    let fixture_path = Path::new("../../tests/fixtures/golden_locations/unicode_and_multiline.py");
    let content = fs::read_to_string(fixture_path)
        .or_else(|_| fs::read_to_string("tests/fixtures/golden_locations/unicode_and_multiline.py"))
        .expect("read python golden fixture");

    let imports = parse_python_imports(&content, fixture_path);

    // 1. Top-level import math (after Unicode emoji lines)
    let math_imp = imports
        .iter()
        .find(|i| i.module == "math")
        .expect("import math found");
    assert_eq!(math_imp.line, 4);
    let math_span = math_imp.span.expect("math span");
    assert_eq!(math_span.start_line, 4);
    assert_eq!(math_span.end_line, 4);

    // 2. Nested multiline import inside calculate_something():
    // from sys import (
    //     version_info,
    //     platform,
    // )
    let ver_imp = imports
        .iter()
        .find(|i| i.module == "sys" && i.symbol.as_deref() == Some("version_info"))
        .expect("import version_info");
    assert_eq!(ver_imp.line, 9);
    let ver_span = ver_imp.span.expect("version_info span");
    assert_eq!(ver_span.start_line, 9);
    assert_eq!(ver_span.end_line, 9);

    let plat_imp = imports
        .iter()
        .find(|i| i.module == "sys" && i.symbol.as_deref() == Some("platform"))
        .expect("import platform");
    assert_eq!(plat_imp.line, 10);
    let plat_span = plat_imp.span.expect("platform span");
    assert_eq!(plat_span.start_line, 10);
    assert_eq!(plat_span.end_line, 10);

    // 3. Nested import inside ExampleClass.method():
    let os_imp = imports
        .iter()
        .find(|i| i.module == "os")
        .expect("import os");
    assert_eq!(os_imp.line, 17);
    let os_span = os_imp.span.expect("os span");
    assert_eq!(os_span.start_line, 17);
    assert_eq!(os_span.end_line, 17);

    // Snippet extraction on multiline / Unicode content
    let snippet = DiagnosticLocation::extract_snippet_from_source(&content, &plat_span);
    assert!(snippet.is_some());
    assert!(snippet.unwrap().contains("platform"));
}
