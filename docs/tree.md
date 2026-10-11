# Dependency & Architecture Tree

`stackvet` provides visual representations of both your package dependency tree and your internal source module import architecture.

---

## 1. Basic Usage

```bash
# Render full project view (dependencies + import graph)
stackvet graph

# Alias for stackvet graph
stackvet graph
```

---

## 2. Visualizing Package Dependencies

To focus exclusively on direct and transitive package dependencies:

```bash
stackvet graph --dependencies
# or shorthand:
stackvet graph -d
```

### 2.1 Limiting Tree Depth

Limit the hierarchy depth using `--depth` / `-L`:

```bash
stackvet graph -d -L 2
```

Example output:

```text
Dependency Hierarchy

fastapi (0.110.0) [direct]
├── starlette (0.36.3) [transitive]
│   └── anyio (4.3.0) [transitive]
│       └── idna (3.6) [transitive]
└── pydantic (2.6.4) [direct]
    ├── annotated-types (0.6.0) [transitive]
    └── pydantic-core (2.16.3) [transitive]
```

---

## 3. Visualizing Internal Module Imports & Circular Loops

To inspect internal Python module imports and pinpoint circular dependency cycles:

```bash
stackvet graph --imports
# or shorthand:
stackvet graph -i
```

Example output:

```text
Internal Module Import Architecture

services.payment
├── models.transaction
└── core.config

services.auth
├── utils.security
└── [🔴 CIRCULAR CYCLE] services.payment -> services.auth
```

---

## 4. Dependency Risk Graph (`stackvet dependency --graph` / `stackvet graph --risk`)

Take dependency trees further by analyzing **usage patterns, functional classifications, and diagnostic risk levels** across direct and transitive dependencies:

```bash
# Render annotated dependency risk graph
stackvet dependency --graph

# Or via tree with risk flag
stackvet graph --risk
```

### 4.1 Node Annotations & Classification

The Dependency Risk Graph classifies each package into its ecosystem role:
- **Framework Core**: e.g., `fastapi`, `celery`, `django`, `sqlalchemy`, `flask`
- **Runtime Driver / Infrastructure**: e.g., `amqp`, `kombu`, `asyncpg`, `psycopg2`, `redis`, `uvicorn`
- **Application Library**: e.g., `requests`, `httpx`, `pydantic`, `rich`
- **Dev & Tooling**: e.g., `pytest`, `ruff`, `mypy`, `black`

Example Output:
```text
🛡️  Dependency Risk Graph: payment-service (3 direct, 48 total packages)
└── Production Dependencies
    └── celery v5.4.0 (>=5.3.0)  HEALTHY
        ├── Type: Direct dependency
        ├── Import: Directly imported (4 files)
        ├── Role: Framework core
        ├── Assessment: No diagnostic issues found; dependency is healthy
        └── kombu v5.3.5  HEALTHY
            └── amqp v5.2.0  INFORMATIONAL
                ├── Type: Transitive dependency
                ├── Import: No direct AST import found
                ├── Role: Runtime driver / message broker
                └── Assessment: Safe runtime dependency (DEP-003 informational)
```

### 4.2 Filtering by Risk Level

Filter the dependency graph by risk threshold:
```bash
# Show only high and critical risk dependencies
stackvet dependency --graph --risk high

# Show only healthy packages
stackvet dependency --graph --risk healthy
```

### 4.3 Compact Mode & JSON Export

```bash
# Render risk tree without detailed multi-line annotations
stackvet dependency --graph --no-annotate

# Export full risk graph metadata as JSON
stackvet dependency --graph --json
```

---

## 5. JSON Export for Tooling

To emit the tree hierarchy, package statistics, and detected cycles as structured JSON for CI ingestion or custom dashboards:

```bash
stackvet graph --json > graph.json
```
