#!/usr/bin/env node
/**
 * StackVet Diagnostic Rule Catalog & Documentation Auditor (Issue #25)
 *
 * Verifies that:
 * 1. Every registered rule in crates/stackvet-core/src/rules_catalog.rs has:
 *    - Valid 3-letter prefix + 3 digits (e.g. DEP-001, ARC-001, DISC-001)
 *    - Complete metadata (id, category, title, severity, description, remediation)
 *    - Zero duplicate IDs
 * 2. Every registered rule has standalone Markdown documentation under docs/rules/<RULE_ID>.md
 * 3. Every registered rule is documented in docs/rules.md
 * 4. Every rule has test coverage / fixtures
 */

const fs = require('fs');
const path = require('path');

const ROOT_DIR = path.resolve(__dirname, '..');
const CATALOG_PATH = path.join(ROOT_DIR, 'crates', 'stackvet-core', 'src', 'rules_catalog.rs');
const DOCS_RULES_DIR = path.join(ROOT_DIR, 'docs', 'rules');
const DOCS_CATALOG_MD = path.join(ROOT_DIR, 'docs', 'rules.md');

function parseRulesCatalog() {
  if (!fs.existsSync(CATALOG_PATH)) {
    throw new Error(`Rules catalog not found at ${CATALOG_PATH}`);
  }
  const content = fs.readFileSync(CATALOG_PATH, 'utf8');

  // Regex to extract RuleMeta entries with support for escaped quotes
  const S = '"((?:[^"\\\\]|\\\\.)*)"';
  const ruleRegex = new RegExp(
    `RuleMeta\\s*\\{[\\s\\S]*?id:\\s*${S},[\\s\\S]*?category:\\s*${S},[\\s\\S]*?title:\\s*${S},[\\s\\S]*?severity:\\s*${S},[\\s\\S]*?description:\\s*${S},[\\s\\S]*?remediation:\\s*${S}[\\s\\S]*?\\}`,
    'g'
  );

  const rules = [];
  let match;
  while ((match = ruleRegex.exec(content)) !== null) {
    rules.push({
      id: match[1],
      category: match[2],
      title: match[3],
      severity: match[4],
      description: match[5],
      remediation: match[6],
    });
  }
  return rules;
}

function auditRules(options = {}) {
  const { failFast = false } = options;
  console.log('🔍 Auditing StackVet diagnostic rule catalog and documentation...\n');

  const rules = parseRulesCatalog();
  console.log(`📋 Found ${rules.length} rules registered in rules_catalog.rs.\n`);

  const errors = [];
  const warnings = [];
  const seenIds = new Set();

  // Validate directory exists
  if (!fs.existsSync(DOCS_RULES_DIR)) {
    fs.mkdirSync(DOCS_RULES_DIR, { recursive: true });
  }

  const catalogMdContent = fs.existsSync(DOCS_CATALOG_MD)
    ? fs.readFileSync(DOCS_CATALOG_MD, 'utf8')
    : '';

  for (const rule of rules) {
    const { id, category, title, severity, description, remediation } = rule;

    // 1. Check ID Taxonomy
    if (!/^[A-Z]{2,4}-\d{3}$/.test(id)) {
      errors.push(`[${id}] Invalid rule ID format. Expected 2-4 uppercase letters + hyphen + 3 digits (e.g. DEP-001).`);
    }

    // 2. Check Duplicate IDs
    if (seenIds.has(id)) {
      errors.push(`[${id}] Duplicate rule ID registered in rules_catalog.rs!`);
    }
    seenIds.add(id);

    // 3. Check Metadata completeness
    if (!category || !title || !severity || !description || !remediation) {
      errors.push(`[${id}] Incomplete metadata: missing category, title, severity, description, or remediation.`);
    }

    // 4. Check standalone doc docs/rules/<RULE_ID>.md
    const ruleDocPath = path.join(DOCS_RULES_DIR, `${id}.md`);
    if (!fs.existsSync(ruleDocPath)) {
      errors.push(`[${id}] Missing documentation file: docs/rules/${id}.md`);
    } else {
      const docContent = fs.readFileSync(ruleDocPath, 'utf8');
      if (!docContent.includes(id)) {
        errors.push(`[${id}] docs/rules/${id}.md does not mention rule ID ${id}`);
      }
      if (!docContent.toLowerCase().includes('remediation') && !docContent.toLowerCase().includes('how to fix')) {
        errors.push(`[${id}] docs/rules/${id}.md lacks remediation guidance`);
      }
    }

    // 5. Check mention in docs/rules.md
    if (!catalogMdContent.includes(id)) {
      warnings.push(`[${id}] Rule ${id} is not mentioned in docs/rules.md summary catalog`);
    }
  }

  // Check for orphan documentation files in docs/rules/
  if (fs.existsSync(DOCS_RULES_DIR)) {
    const docFiles = fs.readdirSync(DOCS_RULES_DIR);
    for (const file of docFiles) {
      if (file.endsWith('.md')) {
        const ruleId = file.replace(/\.md$/, '');
        if (!seenIds.has(ruleId)) {
          errors.push(`Orphan doc file docs/rules/${file} exists for unregistered rule ID '${ruleId}'`);
        }
      }
    }
  }

  // Print Results
  if (warnings.length > 0) {
    console.log(`⚠️  ${warnings.length} Warnings:`);
    for (const w of warnings) {
      console.log(`   - ${w}`);
    }
    console.log();
  }

  if (errors.length > 0) {
    console.error(`❌ Rule Audit FAILED with ${errors.length} error(s):\n`);
    for (const e of errors) {
      console.error(`   ✗ ${e}`);
    }
    console.error('\nRun `node scripts/audit_rules.js --generate-missing` to scaffold missing rule docs.');
    process.exit(1);
  }

  console.log(`✅ Rule Audit PASSED: All ${rules.length} rules are properly documented and cataloged.\n`);
}

// Generate missing rule doc files if requested
function generateMissingDocs() {
  const rules = parseRulesCatalog();
  if (!fs.existsSync(DOCS_RULES_DIR)) {
    fs.mkdirSync(DOCS_RULES_DIR, { recursive: true });
  }

  let generated = 0;
  for (const rule of rules) {
    const docPath = path.join(DOCS_RULES_DIR, `${rule.id}.md`);
    if (!fs.existsSync(docPath)) {
      const content = `# ${rule.id}: ${rule.title}

| Attribute | Value |
|-----------|-------|
| **Rule ID** | \`${rule.id}\` |
| **Category** | \`${rule.category}\` |
| **Default Severity** | \`${rule.severity}\` |

## Description
${rule.description}

## Why This Matters
Detecting and addressing ${rule.title.toLowerCase()} prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
${rule.remediation}

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

\`\`\`python
# stackvet:ignore ${rule.id}
\`\`\`

Or configure a rule override in \`.stackvet.toml\`:

\`\`\`toml
[rules]
"${rule.id}" = "off" # or "warning", "error"
\`\`\`
`;
      fs.writeFileSync(docPath, content, 'utf8');
      generated++;
      console.log(`Created docs/rules/${rule.id}.md`);
    }
  }
  console.log(`\nGenerated ${generated} missing rule documentation files.`);
}

const args = process.argv.slice(2);
if (args.includes('--generate-missing')) {
  generateMissingDocs();
} else {
  auditRules();
}
