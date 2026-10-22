use stackvet_core::{validate_rule, RuleMeta, RuleValidationError, ALL_RULES};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_catalog_rule_integrity_and_uniqueness() {
    assert!(
        ALL_RULES.len() >= 100,
        "Expected at least 100 rules in catalog, found {}",
        ALL_RULES.len()
    );

    let mut seen_ids = HashSet::new();
    for rule in ALL_RULES {
        let is_unique = seen_ids.insert(rule.id);
        assert!(
            is_unique,
            "Duplicate rule ID found in rules catalog: {}",
            rule.id
        );

        if let Err(err) = validate_rule(rule) {
            panic!("Rule {} failed integrity validation: {}", rule.id, err);
        }
    }
}

#[test]
fn test_rule_validation_errors() {
    // Invalid ID taxonomy formats
    let invalid_ids = [
        "INVALID",
        "DEP",
        "DEP001",
        "D-001",
        "TOOLONG-001",
        "dep-001",
        "DEP-1",
        "DEP-12",
        "DEP-1234",
    ];
    for bad_id in invalid_ids {
        let rule = RuleMeta {
            id: bad_id,
            category: "test",
            title: "Test rule",
            severity: "ERROR",
            description: "A test rule description",
            remediation: "A test rule remediation",
        };
        match validate_rule(&rule) {
            Err(RuleValidationError::InvalidIdFormat(id)) => assert_eq!(id, bad_id),
            other => panic!("Expected InvalidIdFormat for '{}', got {:?}", bad_id, other),
        }
    }

    // Empty fields
    let rule_empty_category = RuleMeta {
        id: "TEST-001",
        category: "  ",
        title: "Test rule",
        severity: "ERROR",
        description: "A test rule description",
        remediation: "A test rule remediation",
    };
    assert_eq!(
        validate_rule(&rule_empty_category),
        Err(RuleValidationError::EmptyCategory)
    );

    let rule_empty_title = RuleMeta {
        id: "TEST-001",
        category: "test",
        title: "  ",
        severity: "ERROR",
        description: "A test rule description",
        remediation: "A test rule remediation",
    };
    assert_eq!(
        validate_rule(&rule_empty_title),
        Err(RuleValidationError::EmptyTitle)
    );

    let rule_empty_desc = RuleMeta {
        id: "TEST-001",
        category: "test",
        title: "Test rule",
        severity: "ERROR",
        description: "",
        remediation: "A test rule remediation",
    };
    assert_eq!(
        validate_rule(&rule_empty_desc),
        Err(RuleValidationError::EmptyDescription)
    );

    let rule_empty_remediation = RuleMeta {
        id: "TEST-001",
        category: "test",
        title: "Test rule",
        severity: "ERROR",
        description: "A test rule description",
        remediation: " \t ",
    };
    assert_eq!(
        validate_rule(&rule_empty_remediation),
        Err(RuleValidationError::EmptyRemediation)
    );

    let rule_bad_severity = RuleMeta {
        id: "TEST-001",
        category: "test",
        title: "Test rule",
        severity: "FATAL_CRASH",
        description: "A test rule description",
        remediation: "A test rule remediation",
    };
    assert_eq!(
        validate_rule(&rule_bad_severity),
        Err(RuleValidationError::InvalidSeverity(
            "FATAL_CRASH".to_string()
        ))
    );
}

#[test]
fn test_all_rules_have_documentation_files() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("Failed to determine repository root");
    let docs_rules_dir = repo_root.join("docs").join("rules");

    assert!(
        docs_rules_dir.is_dir(),
        "Expected docs/rules directory to exist at {:?}",
        docs_rules_dir
    );

    for rule in ALL_RULES {
        let doc_path = docs_rules_dir.join(format!("{}.md", rule.id));
        assert!(
            doc_path.is_file(),
            "Documentation file missing for rule {}: expected at {:?}",
            rule.id,
            doc_path
        );

        let content = fs::read_to_string(&doc_path)
            .unwrap_or_else(|e| panic!("Failed to read doc file for {}: {}", rule.id, e));

        assert!(
            content.contains(&format!("# {}:", rule.id))
                || content.contains(&format!("# {}", rule.id)),
            "Doc file {:?} does not contain heading with rule ID {}",
            doc_path,
            rule.id
        );
    }
}
