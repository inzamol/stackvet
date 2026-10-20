use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use stackvet_core::{Diagnostic, DiagnosticLocation, LanguageTarget, RuleId, Severity, TextSpan};
use stackvet_scanner::{
    parse_explicit_files, parse_name_status, parse_patch_file, parse_unified_diff_hunks,
    ChangedFile, FileDiffKind, PrAnalysisResult,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn make_temp_dir(name: &str) -> PathBuf {
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "stackvet_test_diff_{}_{}_{}_{}",
        name,
        std::process::id(),
        unique,
        count
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_parse_name_status_variants() {
    let root = Path::new("/workspace");
    let output =
        "M\tsrc/main.py\nA\tsrc/new_file.py\nD\tsrc/old.py\nR100\told_name.py\tnew_name.py\n";
    let files = parse_name_status(output, root);
    assert_eq!(files.len(), 4);

    assert_eq!(files[0].path, root.join("src/main.py"));
    assert_eq!(files[0].kind, FileDiffKind::Modified);

    assert_eq!(files[1].path, root.join("src/new_file.py"));
    assert_eq!(files[1].kind, FileDiffKind::Added);

    assert_eq!(files[2].path, root.join("src/old.py"));
    assert_eq!(files[2].kind, FileDiffKind::Deleted);

    assert_eq!(files[3].path, root.join("new_name.py"));
    assert_eq!(
        files[3].kind,
        FileDiffKind::Renamed {
            from: root.join("old_name.py")
        }
    );
}

#[test]
fn test_parse_unified_diff_hunks() {
    let root = Path::new("/workspace");
    let diff = r#"diff --git a/src/lib.py b/src/lib.py
index 1234567..89abcdef 100644
--- a/src/lib.py
+++ b/src/lib.py
@@ -10,6 +10,8 @@ def existing():
     pass
+def added_one():
+    pass
@@ -50,4 +52,7 @@ def another():
-    pass
+    # replaced
+    return True
+    # added line
"#;
    let hunks = parse_unified_diff_hunks(diff, root);
    assert_eq!(hunks.len(), 1);

    let target_path = root.join("src/lib.py");
    let lib_hunks = hunks.get(&target_path).expect("src/lib.py hunks found");
    assert_eq!(lib_hunks.len(), 2);

    assert_eq!(lib_hunks[0], 10..=17);
    assert_eq!(lib_hunks[1], 52..=58);
}

#[test]
fn test_parse_explicit_files_and_patch() {
    let temp = make_temp_dir("explicit_files");
    let file1 = temp.join("foo.py");
    let file2 = temp.join("bar.py");
    fs::write(&file1, "print('foo')\n").unwrap();
    fs::write(&file2, "print('bar')\n").unwrap();

    let files_arg = "foo.py,bar.py";
    let parsed = parse_explicit_files(files_arg, &temp);
    assert_eq!(parsed.len(), 2);
    assert!(parsed
        .iter()
        .any(|f| f.path == temp.join("foo.py") && f.kind == FileDiffKind::Modified));
    assert!(parsed
        .iter()
        .any(|f| f.path == temp.join("bar.py") && f.kind == FileDiffKind::Modified));

    let patch = temp.join("sample.patch");
    let patch_content = r#"diff --git a/foo.py b/foo.py
--- a/foo.py
+++ b/foo.py
@@ -1,1 +1,2 @@
 print('foo')
+print('more')
"#;
    fs::write(&patch, patch_content).unwrap();
    let patch_files = parse_patch_file(&patch, &temp).unwrap();
    assert_eq!(patch_files.len(), 1);
    assert_eq!(patch_files[0].path, temp.join("foo.py"));
    assert_eq!(patch_files[0].changed_lines, vec![1..=2]);
}

#[test]
fn test_pr_analysis_categorizes_new_existing_and_resolved() {
    let root = Path::new("/workspace");
    let modified_path = root.join("src/modified.py");
    let untouched_path = root.join("src/untouched.py");
    let other_path = root.join("src/other.py");

    let mut changed_file = ChangedFile::new(modified_path.clone(), FileDiffKind::Modified);
    #[allow(clippy::single_range_in_vec_init)]
    {
        changed_file.changed_lines = vec![10..=15];
    }

    let changed_files = vec![
        changed_file,
        ChangedFile::new(untouched_path.clone(), FileDiffKind::Modified),
    ];

    let d_new_in_diff = Diagnostic {
        rule_id: RuleId::new("DEP-001"),
        message: "Undeclared import in changed hunk".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new(modified_path.clone())
            .with_span(TextSpan::single_line(12, 1, 10)),
        language: LanguageTarget::Python,
        ..Default::default()
    };

    let d_outside_diff = Diagnostic {
        rule_id: RuleId::new("DEP-002"),
        message: "Existing violation outside diff hunk".to_string(),
        severity: Severity::Warning,
        location: DiagnosticLocation::new(modified_path.clone())
            .with_span(TextSpan::single_line(99, 1, 10)),
        language: LanguageTarget::Python,
        ..Default::default()
    };

    let d_in_untouched = Diagnostic {
        rule_id: RuleId::new("DEP-003"),
        message: "Existing violation in untouched file".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new(other_path).with_span(TextSpan::single_line(5, 1, 10)),
        language: LanguageTarget::Python,
        ..Default::default()
    };

    let d_resolved_from_baseline = Diagnostic {
        rule_id: RuleId::new("DEP-004"),
        message: "Old violation that is now gone".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new(modified_path)
            .with_span(TextSpan::single_line(11, 1, 10)),
        language: LanguageTarget::Python,
        ..Default::default()
    };

    let base_diagnostics = vec![
        d_outside_diff.clone(),
        d_in_untouched.clone(),
        d_resolved_from_baseline.clone(),
    ];

    let head_diagnostics = vec![
        d_new_in_diff.clone(),
        d_outside_diff.clone(),
        d_in_untouched.clone(),
    ];

    let result = PrAnalysisResult::analyze(changed_files, &head_diagnostics, &base_diagnostics);

    assert_eq!(result.new_diagnostics.len(), 1);
    assert_eq!(result.new_diagnostics[0].rule_id.as_str(), "DEP-001");

    assert_eq!(result.existing_diagnostics.len(), 2);
    let existing_ids: Vec<_> = result
        .existing_diagnostics
        .iter()
        .map(|d| d.rule_id.as_str())
        .collect();
    assert!(existing_ids.contains(&"DEP-002"));
    assert!(existing_ids.contains(&"DEP-003"));

    assert_eq!(result.resolved_diagnostics.len(), 1);
    assert_eq!(result.resolved_diagnostics[0].rule_id.as_str(), "DEP-004");
}

#[test]
fn test_pr_analysis_github_annotations_and_markdown() {
    let d1 = Diagnostic {
        rule_id: RuleId::new("DEP-001"),
        message: "New undeclared import".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new("src/app.py").with_span(TextSpan::single_line(25, 4, 10)),
        language: LanguageTarget::Python,
        ..Default::default()
    };

    let result = PrAnalysisResult::analyze(vec![], &[d1], &[]);

    let annotations = result.render_github_annotations();
    let annotations_joined = annotations.join("\n");
    assert!(annotations_joined
        .contains("::error file=src/app.py,line=25,col=4,title=DEP-001::New undeclared import"));

    let markdown = result.render_markdown_comment();
    assert!(markdown.contains("### 🔍 StackVet Pull Request Analysis"));
    assert!(markdown.contains("DEP-001"));
}

#[test]
#[allow(clippy::single_range_in_vec_init)]
fn test_parse_unified_diff_hunks_quoted_paths() {
    let root = Path::new("/workspace");
    let diff = r#"diff --git "a/path with space/file.py" "b/path with space/file.py"
--- "a/path with space/file.py"
+++ "b/path with space/file.py"
@@ -5,3 +5,4 @@ def test():
     pass
+    # new line
"#;
    let hunks = parse_unified_diff_hunks(diff, root);
    let target = root.join("path with space/file.py");
    assert!(
        hunks.contains_key(&target),
        "Expected key for unquoted relative path"
    );
    assert_eq!(hunks.get(&target).unwrap().as_slice(), &[5..=8]);
}

#[test]
fn test_github_annotations_escaping() {
    let d = Diagnostic {
        rule_id: RuleId::new("RULE:1,2"),
        message: "Message with % and \n newlines".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new("src/path:name,test.py")
            .with_span(TextSpan::single_line(10, 2, 5)),
        language: LanguageTarget::Python,
        ..Default::default()
    };

    let result = PrAnalysisResult::analyze(vec![], &[d], &[]);
    let annotations = result.render_github_annotations();
    let text = annotations.join("\n");

    assert!(text.contains("file=src/path%3Aname%2Ctest.py"));
    assert!(text.contains("title=RULE%3A1%2C2"));
    assert!(text.contains("Message with %25 and %0A newlines"));
}
