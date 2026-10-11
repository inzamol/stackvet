# Diagnostic Rules Catalog

`stackvet` rule IDs are stable public identifiers. This catalog lists all supported diagnostic checks, their default severities, and remediation strategies.

---

## 1. Dependency Rules (`DEP-xxx`)

### 1.1 DEP-001: Dependency Constraint Conflict
- **Default Severity:** `ERROR`
- **Description:** Two or more declared or transitive dependencies have incompatible version constraints.
- **Remediation:** Upgrade the conflicting package or loosen the version pin to satisfy all requirements.

### 1.2 DEP-002: Missing Dependency Declaration
- **Default Severity:** `ERROR`
- **Description:** A third-party package is imported in source code but is not declared in project dependencies.
- **Remediation:** Add the missing package to your pyproject.toml or requirements.txt.

### 1.3 DEP-003: No Direct Import Detected
- **Default Severity:** `WARNING`
- **Description:** A package is declared as a direct dependency, but no direct imports were detected across project source files. This detects the absence of a detected direct import, not proof that the dependency is unused. Dependencies may still be required at runtime through plugins, configuration, framework integration, CLI entrypoints, or indirect mechanisms.
- **Remediation:** Verify if this dependency is used dynamically, required as a plugin/driver/runtime configuration, or can be safely removed.

### 1.4 DEP-004: Python Compatibility Mismatch
- **Default Severity:** `WARNING`
- **Description:** A package requires a Python version incompatible with the target project runtime.
- **Remediation:** Update your target Python version or install a version of the package compatible with your Python runtime.

### 1.5 DEP-005: Installed/Declaration Mismatch
- **Default Severity:** `WARNING`
- **Description:** The package version installed in the virtualenv does not match the pinned requirement in project declaration.
- **Remediation:** Sync your virtual environment using your package manager (e.g. `uv sync` or `poetry install`).

### 1.6 DEP-006: Vulnerable Transitive Dependency
- **Default Severity:** `ERROR`
- **Description:** A security vulnerability has been identified in a direct or transitive dependency.
- **Remediation:** Update the vulnerable dependency or apply vendor security patches.

### 1.7 DEP-007: Undeclared Transitive Dependency
- **Default Severity:** `WARNING`
- **Description:** A package is imported in source code and provided transitively by a dependency, but is not declared directly in project dependencies.
- **Remediation:** Add the transitive dependency directly to your pyproject.toml or requirements.txt.


---

## 2. Security Rules (`SEC-xxx`)

### 2.1 SEC-001: Security Analysis Unavailable
- **Default Severity:** `WARNING`
- **Description:** The advisory database (OSV.dev) could not be reached to perform vulnerability checks.
- **Remediation:** Check your internet connection or run with --offline to skip security checks.

---

## 3. Environment & Drift Rules (`ENV-xxx`)

### 3.1 ENV-001: Python Version Drift
- **Default Severity:** `WARNING`
- **Description:** The active Python interpreter version differs from the project's target runtime specification.
- **Remediation:** Ensure the virtual environment is built using the configured target Python version.

### 3.2 ENV-002: Docker Runtime Drift
- **Default Severity:** `WARNING`
- **Description:** The Python version in Dockerfile/docker-compose differs from the project's target runtime.
- **Remediation:** Update the base image tag in your Dockerfile to match your project's target Python version.

### 3.3 ENV-003: CI Runtime Drift
- **Default Severity:** `WARNING`
- **Description:** CI workflow test matrix does not cover or differs from declared project Python versions.
- **Remediation:** Align your CI matrix with the Python versions specified in pyproject.toml.

---

## 4. Import & Architecture Rules (`IMP-xxx`)

### 4.1 IMP-001: Circular Import Detected
- **Default Severity:** `ERROR`
- **Description:** An import cycle exists between two or more modules, which may cause runtime initialization errors.
- **Remediation:** Refactor shared dependencies into a separate module or use deferred/lazy imports.

### 4.2 IMP-002: Unresolved Local Import
- **Default Severity:** `ERROR`
- **Description:** A local module imported in source code could not be resolved on the project's Python path.
- **Remediation:** Verify the module name and ensure source directory is marked on the PYTHONPATH or package root.

### 4.3 IMP-003: Unused / Orphan Local Module
- **Default Severity:** `WARNING`
- **Description:** A Python source file exists in the project but is never imported or referenced by any other module, entry point, or test file.
- **Remediation:** Review if this module is obsolete and can be safely deleted or integrated into your package exports.

### 4.4 IMP-004: Deprecated or Removed Standard Library Module
- **Default Severity:** `ERROR`
- **Description:** A standard library module imported in source code was deprecated or completely removed in modern Python (PEP 594).
- **Remediation:** Replace the removed stdlib module with its modern replacement (e.g. importlib instead of imp, subprocess instead of pipes).

### 4.5 ARC-001: Architecture Layer Violation
- **Default Severity:** `WARNING`
- **Description:** A module in an upper architectural layer directly imports a lower layer bypassing intermediate abstraction layers (e.g. API importing Database directly instead of through Services/Repositories), or a lower layer imports an upper layer.
- **Remediation:** Route access through the designated intermediate service/repository layer or invert the dependency using interfaces/events.

### 4.6 ARC-002: Architectural Circular Component Dependency
- **Default Severity:** `ERROR`
- **Description:** A circular dependency cycle exists between high-level architectural components or layers (e.g. Services → Utils → Services).
- **Remediation:** Decouple the architectural components by extracting shared utilities, introducing an abstraction layer, or using dependency injection.

---

## 5. Packaging Rules (`PKG-xxx`)

### 5.1 PKG-001: Missing Package Metadata
- **Default Severity:** `WARNING`
- **Description:** Essential packaging metadata (such as project name, version, or description) is missing.
- **Remediation:** Provide project name and version in pyproject.toml [project] table.

### 5.2 PKG-002: Invalid Project Configuration
- **Default Severity:** `ERROR`
- **Description:** The project configuration file contains syntax errors or invalid keys.
- **Remediation:** Fix configuration syntax to conform to PEP 621 / tool specifications.

---

## 6. FastAPI Framework Rules (`FAP-xxx`)

### 6.1 FAP-001: Blocking Call or CPU-Bound Operation in Async Endpoint
- **Default Severity:** `ERROR`
- **Description:** Synchronous blocking operations (e.g. time.sleep, requests, subprocess, synchronous DB queries) or CPU-bound hashing called inside an async def FastAPI route handler block the asyncio event loop.
- **Remediation:** Use non-blocking async alternatives (e.g. asyncio.sleep, httpx.AsyncClient) or run blocking/CPU calls in worker threads via anyio.to_thread.run_sync or fastapi.concurrency.run_in_threadpool.

### 6.2 FAP-002: Blocking Dependency in Async Path
- **Default Severity:** `WARNING`
- **Description:** An async route handler depends on a dependency (via Depends) that executes blocking synchronous I/O or CPU operations.
- **Remediation:** Refactor dependency to use async operations or execute in a worker threadpool.

### 6.3 FAP-003: Missing Response Model or Return Type Annotation
- **Default Severity:** `WARNING`
- **Description:** FastAPI route endpoint does not declare a response_model argument or return type annotation, disabling schema validation and response serialization.
- **Remediation:** Add a return type annotation (-> ResponseModel) or pass response_model to the route decorator.

### 6.4 FAP-004: Insecure CORS or Debug Configuration
- **Default Severity:** `ERROR`
- **Description:** CORSMiddleware configured with allow_origins=['*'] and allow_credentials=True, or FastAPI instantiated with debug=True.
- **Remediation:** Specify explicit trusted origins in allow_origins when credentials are enabled, and disable debug mode in production.

### 6.5 FAP-005: Missing HTTP Client Timeout Configuration
- **Default Severity:** `WARNING`
- **Description:** HTTP client (such as httpx.AsyncClient, httpx.Client, or aiohttp.ClientSession) instantiated without an explicit timeout.
- **Remediation:** Configure an explicit timeout (e.g. timeout=10.0) to prevent hanging connections.

### 6.6 FAP-006: Route Path Parameter Template Mismatch
- **Default Severity:** `ERROR`
- **Description:** A path parameter defined in the route URL template (e.g. '/users/{user_id}') does not match any parameter in the handler function signature.
- **Remediation:** Ensure all {param} placeholders in route URL decorators match parameter names in the endpoint function signature.

### 6.7 FAP-007: Unsafe Yield Dependency Without Try/Finally
- **Default Severity:** `WARNING`
- **Description:** A generator dependency using yield does not wrap cleanup code in a try...finally block, causing resource and connection leaks on exceptions.
- **Remediation:** Wrap the yield and teardown logic in a try...finally block to guarantee resource cleanup.

### 6.8 FAP-008: Deprecated @app.on_event Lifecycle Hook
- **Default Severity:** `WARNING`
- **Description:** Using deprecated @app.on_event('startup' / 'shutdown') handlers instead of modern lifespan context managers.
- **Remediation:** Migrate @app.on_event to '@asynccontextmanager async def lifespan(app: FastAPI)' and pass to FastAPI(lifespan=lifespan).

### 6.9 FAP-009: Untyped or Unvalidated Request Body
- **Default Severity:** `WARNING`
- **Description:** Route handler accepts a raw 'dict', 'Any', or untyped Body parameter instead of a Pydantic model, bypassing automatic request validation.
- **Remediation:** Define a Pydantic BaseModel schema for the request payload to ensure type safety and validation.

### 6.10 FAP-010: Shadowed or Duplicate Route Endpoint
- **Default Severity:** `ERROR`
- **Description:** Multiple route handlers are registered with the identical HTTP method and path on the same app/router, causing one to shadow the other.
- **Remediation:** Ensure all routes have unique method and path combinations, or combine related logic into a single handler.

### 6.11 FAP-011: Mutable Default Value in Route Parameter
- **Default Severity:** `WARNING`
- **Description:** Route handler uses a mutable default argument (e.g. list or dict), which can leak state across concurrent requests.
- **Remediation:** Use None as default (e.g. filters: list[str] | None = None) or use Field(default_factory=list).

### 6.12 FAP-012: Invalid Exception Handler Signature
- **Default Severity:** `ERROR`
- **Description:** Custom @app.exception_handler function does not take the required (request, exc) parameters, causing runtime crashes.
- **Remediation:** Ensure custom exception handlers accept exactly two parameters: '(request: Request, exc: Exception)'.

### 6.13 FAP-013: Untracked asyncio.create_task in Route Handler
- **Default Severity:** `WARNING`
- **Description:** Spawning raw asyncio.create_task inside route handlers without background task management can cause lost exceptions and unhandled crashes.
- **Remediation:** Use FastAPI's built-in BackgroundTasks ('background_tasks.add_task(...)') for request-scoped background jobs.

### 6.14 FAP-014: Potential Path Traversal in FileResponse
- **Default Severity:** `WARNING`
- **Description:** FileResponse initialized directly with an unvalidated path variable from route parameters, risking arbitrary file read vulnerabilities.
- **Remediation:** Validate and resolve the file path against a trusted base directory before passing to FileResponse.

### 6.15 FAP-015: Blocking File I/O in Async Endpoint
- **Default Severity:** `ERROR`
- **Description:** Using synchronous open() or blocking file read/write operations inside an async def route handler blocks the event loop.
- **Remediation:** Use 'aiofiles', 'anyio.Path', or run file I/O in a synchronous def route handler.

### 6.16 FAP-016: Sensitive Field Exposure in Schema
- **Default Severity:** `ERROR`
- **Description:** Pydantic model schema exposes sensitive password/secret fields without Field(exclude=True) or response filtering.
- **Remediation:** Mark sensitive fields with 'Field(exclude=True)' or create a separate output schema (e.g. UserResponse).

### 6.17 FAP-017: Non-Standard HTTP Status Code on POST or DELETE
- **Default Severity:** `WARNING`
- **Description:** Mutation routes (POST/DELETE) return default 200 OK instead of standard REST status codes (201 Created or 204 No Content).
- **Remediation:** Specify status_code=status.HTTP_201_CREATED on POST routes or status.HTTP_204_NO_CONTENT on DELETE routes.

### 6.18 FAP-018: Global In-Memory State Mutation in Route Handler
- **Default Severity:** `ERROR`
- **Description:** Route handler mutates global/module-level collections or variables without synchronization locks, causing concurrency bugs.
- **Remediation:** Use a proper database, Redis cache, or an asyncio.Lock for shared state.

### 6.19 FAP-019: Insecure Cookie Configuration
- **Default Severity:** `ERROR`
- **Description:** response.set_cookie() called without httponly=True or secure=True flags, exposing cookies to XSS or network interception.
- **Remediation:** Set 'httponly=True' and 'secure=True' when storing sensitive session or auth cookies.

### 6.20 FAP-020: Potential Open Redirect in RedirectResponse
- **Default Severity:** `WARNING`
- **Description:** RedirectResponse initialized directly with user-supplied URL parameter without domain whitelist validation.
- **Remediation:** Validate that the redirect target is a relative path or matches an allowed domain whitelist.

### 6.21 FAP-021: Router Included Without Tags or Prefix
- **Default Severity:** `WARNING`
- **Description:** app.include_router() called without 'tags' or 'prefix', leading to unorganized OpenAPI documentation and URL clashes.
- **Remediation:** Provide 'prefix' and 'tags' when including routers (e.g. app.include_router(user_router, prefix='/users', tags=['Users'])).

### 6.22 FAP-022: WebSocket Route Missing Await websocket.accept()
- **Default Severity:** `ERROR`
- **Description:** WebSocket endpoint sends or receives data before calling 'await websocket.accept()', which causes runtime errors.
- **Remediation:** Call 'await websocket.accept()' before reading or writing data to the WebSocket.

### 6.23 FAP-023: Deprecated Pydantic v1 Config Class
- **Default Severity:** `WARNING`
- **Description:** Pydantic schema uses deprecated inner 'class Config:' instead of Pydantic v2 'model_config = ConfigDict(...)'.
- **Remediation:** Migrate inner 'class Config:' to 'model_config = ConfigDict(...)'.

### 6.24 FAP-024: Redundant Duplicate Dependency Declaration
- **Default Severity:** `WARNING`
- **Description:** Route handler declares multiple parameters resolving the exact same dependency callable.
- **Remediation:** Consolidate duplicate dependencies into a single parameter.

### 6.25 FAP-025: Returned HTTPException Instance Instead of Raise
- **Default Severity:** `ERROR`
- **Description:** Returning an HTTPException instance in a route handler returns a 200 OK HTTP response with the serialized exception object instead of raising an error.
- **Remediation:** Use 'raise HTTPException(...)' instead of 'return HTTPException(...)'.

### 6.26 FAP-026: Deprecated Pydantic v1 @validator or @root_validator
- **Default Severity:** `WARNING`
- **Description:** Using deprecated Pydantic v1 '@validator' or '@root_validator' decorators instead of Pydantic v2 '@field_validator' or '@model_validator'.
- **Remediation:** Migrate to '@field_validator' or '@model_validator' from pydantic.

### 6.27 FAP-027: Dependency Parameter Missing Type Annotation
- **Default Severity:** `WARNING`
- **Description:** Route parameter initialized with 'Depends(...)', 'Query(...)', or 'Path(...)' does not specify a type annotation, disabling FastAPI schema generation and static type checking.
- **Remediation:** Add explicit type annotation: 'param: Type = Depends(...)' or 'param: Annotated[Type, Depends(...)]'.

### 6.28 FAP-028: Raw json.loads Called on Request Body
- **Default Severity:** `WARNING`
- **Description:** Calling 'json.loads(await request.body())' or 'json.loads(request.body)' is an anti-pattern; FastAPI and Starlette provide an optimized built-in 'await request.json()' method.
- **Remediation:** Replace 'json.loads(await request.body())' with 'await request.json()'.

### 6.29 FAP-029: StreamingResponse Initialized Without media_type
- **Default Severity:** `WARNING`
- **Description:** Instantiating StreamingResponse without specifying 'media_type' can cause clients to misinterpret the streamed content format.
- **Remediation:** Pass an explicit media_type argument (e.g. StreamingResponse(stream, media_type='application/json')).

### 6.30 FAP-030: Missing Route Summary or Docstring for OpenAPI Documentation
- **Default Severity:** `WARNING`
- **Description:** Public route endpoint does not provide a docstring, 'summary', or 'description', resulting in sparse and incomplete OpenAPI documentation.
- **Remediation:** Add a function docstring or pass 'summary=...' in the route decorator.

### 6.31 FAP-031: Prefer typing.Annotated Over Parameter Default Assignment
- **Default Severity:** `INFO`
- **Description:** FastAPI strongly recommends using standard typing.Annotated[Type, Depends(...)] instead of parameter default values for cleaner typing and reusable dependencies.
- **Remediation:** Refactor parameter to 'param: Annotated[Type, Depends(...)]' or 'param: Annotated[Type, Query(...)]'.

### 6.32 FAP-032: Redundant jsonable_encoder with response_model
- **Default Severity:** `WARNING`
- **Description:** Calling jsonable_encoder() inside a route handler that already declares response_model causes double serialization, reducing throughput.
- **Remediation:** Return raw objects or dicts directly and let FastAPI's response_model handle serialization.

### 6.33 FAP-033: Missing from_attributes=True in ORM Response Schema
- **Default Severity:** `WARNING`
- **Description:** Pydantic response model for ORM/database objects is missing 'model_config = ConfigDict(from_attributes=True)', causing validation failures when serializing ORM instances.
- **Remediation:** Add 'model_config = ConfigDict(from_attributes=True)' to the Pydantic schema.

### 6.34 FAP-034: Missing Await on request.json() or request.body()
- **Default Severity:** `ERROR`
- **Description:** Calling request.json() or request.body() without 'await' inside an async def route handler assigns an un-awaited coroutine instead of parsed data.
- **Remediation:** Add 'await' before 'request.json()' or 'request.body()'.

### 6.35 FAP-035: Raw Exception Raised Instead of HTTPException
- **Default Severity:** `WARNING`
- **Description:** Raising generic Exception, ValueError, or RuntimeError in route handlers triggers uncaught 500 Internal Server Errors instead of structured REST HTTP error responses.
- **Remediation:** Raise 'HTTPException(status_code=..., detail=...)' for predictable API client error responses.

### 6.36 FAP-036: Mutating app.state Inside Route Handler
- **Default Severity:** `WARNING`
- **Description:** Mutating request.app.state or app.state inside per-request route handlers introduces concurrency race conditions. Application state should be initialized in lifespan context managers.
- **Remediation:** Initialize shared state in the lifespan context manager before startup rather than modifying app.state in route handlers.

### 6.37 FAP-037: SecurityScopes Declared with Depends Instead of Security
- **Default Severity:** `WARNING`
- **Description:** SecurityScopes parameter declared in a dependency using Depends(...) will not receive requested scopes. Use Security(dependency, scopes=[...]) instead.
- **Remediation:** Use 'Security(dependency, scopes=[...])' when protecting endpoints with OAuth2 SecurityScopes.

### 6.38 FAP-038: Hardcoded HTTP Status Code Integer
- **Default Severity:** `INFO`
- **Description:** Using integer literals for status_code (e.g. status_code=201) reduces readability compared to named constants like status.HTTP_201_CREATED.
- **Remediation:** Import status from fastapi (or HTTPStatus from http) and use named constants (e.g. status.HTTP_201_CREATED).

---

## 7. SQLAlchemy & SQL Database Rules (`SQL-xxx`)

### 7.1 SQL-001: Potential N+1 Database Query in Loop
- **Default Severity:** `WARNING`
- **Description:** Executing database queries (session.execute, session.query, session.get, or select()) inside for/while loops causes N+1 performance degradation.
- **Remediation:** Batch query objects using 'select(...).where(Model.id.in_(ids))' or eager relationship options (e.g. joinedload/selectinload).

### 7.2 SQL-002: Session Instantiated Without Context Manager or Cleanup
- **Default Severity:** `WARNING`
- **Description:** Instantiating a Session without a with-statement context manager or try...finally session.close() causes database connection pool exhaustion.
- **Remediation:** Use 'with SessionLocal() as session:' or wrap session lifecycle in a context manager / dependency yield.

### 7.3 SQL-003: Synchronous DB Operation in Async Event Loop
- **Default Severity:** `ERROR`
- **Description:** Invoking synchronous database engine calls or synchronous session operations inside an async def function blocks the asyncio event loop.
- **Remediation:** Use SQLAlchemy async extension ('create_async_engine' and 'AsyncSession') with async drivers (e.g. asyncpg, aiosqlite).

### 7.4 SQL-004: Raw SQL String Interpolation (SQL Injection Risk)
- **Default Severity:** `ERROR`
- **Description:** Constructing SQL queries using f-strings or string formatting with text() or cursor.execute() introduces severe SQL injection vulnerabilities.
- **Remediation:** Use bound query parameters with text('SELECT ... WHERE id = :id'), {'id': user_val}.

### 7.5 SQL-005: Legacy SQLAlchemy 1.x session.query() Syntax
- **Default Severity:** `WARNING`
- **Description:** Using legacy session.query(...) instead of modern SQLAlchemy 2.0 select() and session.scalars().
- **Remediation:** Migrate from 'session.query(User).filter(...)' to 'session.scalars(select(User).where(...))'.

### 7.6 SQL-006: Missing Relationship Eager Loading Strategy in Async Session
- **Default Severity:** `WARNING`
- **Description:** Accessing lazy-loaded ORM relationships without explicit joinedload/selectinload in async sessions triggers MissingGreenlet errors.
- **Remediation:** Specify 'options(selectinload(Model.relation))' or set lazy='selectin' on relationship definition.

### 7.7 SQL-007: Uncommitted Transaction in Mutation Function
- **Default Severity:** `WARNING`
- **Description:** Session performs mutations (session.add / session.delete) but never executes session.commit() or enters a transaction with session.begin().
- **Remediation:** Ensure mutations are committed with 'await session.commit()' or executed inside 'with session.begin():'.

### 7.8 SQL-008: create_engine Missing pool_pre_ping Connection Health Check
- **Default Severity:** `WARNING`
- **Description:** Creating a database engine without 'pool_pre_ping=True' can lead to runtime disconnect errors when idle connections are closed by firewalls/servers.
- **Remediation:** Add 'pool_pre_ping=True' to create_engine() / create_async_engine().

### 7.9 SQL-009: expire_on_commit=True in AsyncSession
- **Default Severity:** `WARNING`
- **Description:** Configuring AsyncSession or async_sessionmaker with expire_on_commit=True (the default) causes MissingGreenlet exceptions when accessing committed model attributes.
- **Remediation:** Set 'expire_on_commit=False' when instantiating AsyncSession or async_sessionmaker.

### 7.10 SQL-010: Hardcoded Database Credentials in Connection URL
- **Default Severity:** `ERROR`
- **Description:** Database URL contains plain-text passwords or secret credentials in source code instead of loading from environment variables.
- **Remediation:** Read database connection strings from environment variables (e.g. os.getenv('DATABASE_URL')).

### 7.11 SQL-011: Unbounded SELECT Query Without LIMIT or Pagination
- **Default Severity:** `WARNING`
- **Description:** Calling .all() on queries against database tables without limit() or pagination clauses risks memory exhaustion on large datasets.
- **Remediation:** Add '.limit(limit)' and pagination parameters to query statements.

### 7.12 SQL-012: Relationship Cascade Delete Without ForeignKey ondelete CASCADE
- **Default Severity:** `INFO`
- **Description:** Configuring relationship cascade='all, delete-orphan' without database-level ondelete='CASCADE' on ForeignKey forces slow Python-side row-by-row deletion.
- **Remediation:** Add ondelete='CASCADE' to ForeignKey and passive_deletes=True to relationship for database-level cascading.

### 7.13 SQL-013: Session Flush or Commit Called Inside Loop
- **Default Severity:** `WARNING`
- **Description:** Calling session.flush() or session.commit() inside loops creates high database roundtrip overhead. Use bulk insert or single commit after loop.
- **Remediation:** Move session.commit() outside the loop or use session.execute(insert(Model), batch_data).

### 7.14 SQL-014: Deprecated declarative_base() Function
- **Default Severity:** `WARNING`
- **Description:** Using legacy declarative_base() instead of modern SQLAlchemy 2.0 class Base(DeclarativeBase): pass.
- **Remediation:** Define 'class Base(DeclarativeBase): pass' from sqlalchemy.orm.

### 7.15 SQL-015: SQLite Engine Missing check_same_thread=False in Multi-Threaded Application
- **Default Severity:** `WARNING`
- **Description:** SQLite engine initialized without connect_args={'check_same_thread': False} can cause ProgrammingError in multi-threaded web applications.
- **Remediation:** Pass connect_args={'check_same_thread': False} to create_engine() when using SQLite in web applications.

### 7.16 SQL-016: Missing Database Migration Configuration (Alembic)
- **Default Severity:** `INFO`
- **Description:** SQLAlchemy database models are defined in the project, but no Alembic migrations directory or alembic.ini configuration exists.
- **Remediation:** Initialize database migrations using 'alembic init alembic'.

### 7.17 SQL-017: Mapped[...] Attribute Missing mapped_column() in 2.0 Declarative Model
- **Default Severity:** `WARNING`
- **Description:** Using legacy Column() or raw field assignment with Mapped[...] type annotations instead of SQLAlchemy 2.0's native mapped_column().
- **Remediation:** Replace 'col: Mapped[int] = Column(Integer)' with 'col: Mapped[int] = mapped_column()'.

### 7.18 SQL-018: Excessive Connection Pool Size in Single Application Instance
- **Default Severity:** `WARNING`
- **Description:** create_engine configured with pool_size or max_overflow > 50 in a single instance, risking database connection exhaustion.
- **Remediation:** Keep pool_size between 5 and 20 per instance and use external connection poolers (e.g. pgBouncer) for scaling.

### 7.19 SQL-019: NullPool Configured in Persistent Web Application
- **Default Severity:** `WARNING`
- **Description:** Using NullPool in persistent web servers disables connection pooling and creates high latency TCP connection overhead on every request.
- **Remediation:** Use default QueuePool or AsyncAdaptedQueuePool for persistent web applications; reserve NullPool for AWS Lambda/serverless.

### 7.20 SQL-020: Declarative ORM Model Missing Primary Key Definition
- **Default Severity:** `ERROR`
- **Description:** Declarative ORM model defined without at least one primary key column (primary_key=True), which will cause runtime ORM mapping failures.
- **Remediation:** Define at least one column with 'primary_key=True' or 'mapped_column(primary_key=True)'.

### 7.21 SQL-021: Missing session.rollback() in Database Exception Handler
- **Default Severity:** `WARNING`
- **Description:** Catching exceptions around database queries/mutations without invoking session.rollback() leaves the session in an unusable invalid state.
- **Remediation:** Add 'session.rollback()' in exception handlers or use 'with session.begin():' for automatic rollback on error.

### 7.22 SQL-022: Large Binary or Heavy Text Column Without deferred/load_only Strategy
- **Default Severity:** `INFO`
- **Description:** Model includes LargeBinary or Large Text column without deferred() loading, causing heavy memory consumption during full-table selects.
- **Remediation:** Wrap large blob/text columns with 'deferred(Column(LargeBinary))' or use select(Model).options(load_only(...)).

### 7.23 SQL-023: ForeignKey Column Defined Without index=True
- **Default Severity:** `WARNING`
- **Description:** ForeignKey column created without index=True, causing expensive sequential scans during joins, foreign key lookups, and cascade operations.
- **Remediation:** Add 'index=True' to ForeignKey column definitions (e.g. Column(Integer, ForeignKey('users.id'), index=True)).

### 7.24 SQL-024: Insecure Unencrypted Remote Database Connection URL
- **Default Severity:** `WARNING`
- **Description:** Remote database connection URL targeting external hosts does not specify sslmode=require or ssl=true.
- **Remediation:** Append '?sslmode=require' (PostgreSQL) or '?ssl=true' to production database connection strings.

### 7.25 SQL-025: Deprecated engine.execute() or engine.scalar() Direct Call
- **Default Severity:** `ERROR`
- **Description:** Calling engine.execute() or engine.scalar() directly is removed in SQLAlchemy 2.0.
- **Remediation:** Use explicit connection contexts: 'with engine.connect() as conn: result = conn.execute(stmt)'.

### 7.26 SQL-026: AsyncSession Instantiated Without Async Context Manager or Await close()
- **Default Severity:** `WARNING`
- **Description:** AsyncSession created without 'async with' context manager or explicit 'await session.close()', causing connection leaks in async tasks.
- **Remediation:** Use 'async with AsyncSessionLocal() as session:' or ensure 'await session.close()' in a finally block.

### 7.27 SQL-027: Unsafe Concurrent Numeric Balance or Counter Update Without with_for_update()
- **Default Severity:** `WARNING`
- **Description:** Querying a record and updating numeric counters or balance fields without row-level locking (with_for_update()) causes lost update race conditions.
- **Remediation:** Lock rows with 'select(...).with_for_update()' or use atomic SQL increments 'update(Account).values(balance=Account.balance - amt)'.

### 7.28 SQL-028: Thread-Local scoped_session Used in Async Context
- **Default Severity:** `ERROR`
- **Description:** Using thread-local scoped_session in async applications causes session sharing across coroutines and race conditions.
- **Remediation:** Use async_scoped_session(..., scopefunc=asyncio.current_task) or dependency injection per request.

### 7.29 SQL-029: Declarative ORM Model Missing __tablename__ Definition
- **Default Severity:** `ERROR`
- **Description:** Declarative model inheriting from Base/DeclarativeBase does not define __tablename__ or __table__, causing mapping errors.
- **Remediation:** Define '__tablename__ = "table_name"' on concrete ORM classes (or set '__abstract__ = True' for mixins).

### 7.30 SQL-030: Direct DBAPI raw_connection() Used Without Cleanup
- **Default Severity:** `WARNING`
- **Description:** Calling engine.raw_connection() bypasses connection pool management and leaks connections if not closed explicitly.
- **Remediation:** Always close raw DBAPI connections in a try...finally block: 'raw_conn = engine.raw_connection(); try: ... finally: raw_conn.close()'.

---

## 8. Docker & Container Rules (`DOC-xxx`)

### 8.1 DOC-001: Inefficient Docker Layer Caching
- **Default Severity:** `WARNING`
- **Description:** Source code is copied before package dependencies are installed (`COPY . .` before `RUN pip install ...` or `RUN uv sync`), invalidating the build cache on every code change.
- **Remediation:** Copy dependency manifests (e.g. `pyproject.toml`, `requirements.txt`, `uv.lock`) and install packages before copying the full project directory (`COPY . .`).

### 8.2 DOC-002: Root User Container Execution
- **Default Severity:** `WARNING`
- **Description:** The Dockerfile configures an execution entrypoint (`CMD`/`ENTRYPOINT`) without specifying a non-root `USER`.
- **Remediation:** Create and switch to a non-privileged user (e.g. `RUN useradd -m -u 1000 appuser && USER appuser`) before the container entrypoint.

### 8.3 DOC-003: Unpinned Base Image Tag
- **Default Severity:** `INFO`
- **Description:** The base image uses `:latest` or an unpinned tag, which leads to non-reproducible and unpredictable builds.
- **Remediation:** Pin the base image to a specific version or hash (e.g. `python:3.12-slim-bookworm`).

### 8.4 DOC-004: Missing .dockerignore File
- **Default Severity:** `WARNING`
- **Description:** A Dockerfile exists in the project but no `.dockerignore` file is present, risking baking local virtualenvs, credentials, and temp files into the image.
- **Remediation:** Create a `.dockerignore` file in the repository root excluding `.git`, `.venv`, `__pycache__`, and sensitive files.

### 8.5 DOC-005: Missing --no-cache-dir in pip install
- **Default Severity:** `INFO`
- **Description:** `pip install` inside Dockerfile runs without `--no-cache-dir`, which unnecessarily bloats the container layer with cached wheels.
- **Remediation:** Add `--no-cache-dir` to pip install commands (e.g. `pip install --no-cache-dir -r requirements.txt`).

### 8.6 DOC-006: Sensitive File Copied into Docker Image
- **Default Severity:** `ERROR`
- **Description:** The Dockerfile explicitly copies sensitive files (`.env`, private keys, or credentials) into the container image layer.
- **Remediation:** Remove sensitive files from `COPY` instructions and use Docker BuildKit secrets (`--mount=type=secret`) or runtime environment variables instead.

### 8.7 DOC-007: Missing HEALTHCHECK Instruction
- **Default Severity:** `INFO`
- **Description:** The Dockerfile configures a network service or entrypoint without defining a container `HEALTHCHECK`.
- **Remediation:** Add a `HEALTHCHECK` instruction (e.g. `HEALTHCHECK --interval=30s CMD curl -f http://localhost:8000/health || exit 1`).

### 8.8 DOC-008: Missing PYTHONUNBUFFERED Environment Variable
- **Default Severity:** `INFO`
- **Description:** Python buffers stdout/stderr by default, which delays or prevents real-time container log streaming.
- **Remediation:** Add `ENV PYTHONUNBUFFERED=1` to your Dockerfile to ensure unbuffered real-time log output.

### 8.9 DOC-009: Deprecated MAINTAINER Instruction Used
- **Default Severity:** `INFO`
- **Description:** The `MAINTAINER` instruction is deprecated in Dockerfile syntax in favor of metadata labels.
- **Remediation:** Replace `MAINTAINER <author>` with `LABEL maintainer="<author>"`.

### 8.10 DOC-010: Unnecessary sudo Usage in RUN Instruction
- **Default Severity:** `WARNING`
- **Description:** Commands in Docker `RUN` instructions execute with root privileges by default, making `sudo` redundant and error-prone.
- **Remediation:** Remove `sudo` from `RUN` instructions during image build.

### 8.11 DOC-011: apt-get install Without Cleanup
- **Default Severity:** `INFO`
- **Description:** `apt-get install` inside Dockerfile leaves package lists and recommended dependencies in image layers, bloating image size.
- **Remediation:** Use `apt-get install -y --no-install-recommends <pkgs> && rm -rf /var/lib/apt/lists/*` in the same `RUN` layer.

### 8.12 DOC-012: ADD Instruction Used for Local File Copy
- **Default Severity:** `INFO`
- **Description:** The `ADD` instruction was used to copy local files where `COPY` is preferred for predictability and security.
- **Remediation:** Replace `ADD` with `COPY` unless automatic tar archive extraction or remote URL fetching is explicitly intended.

### 8.13 DOC-013: Missing PYTHONDONTWRITEBYTECODE
- **Default Severity:** `INFO`
- **Description:** Python creates `.pyc` bytecode cache files inside container layers that are usually unnecessary in ephemeral containers.
- **Remediation:** Add `ENV PYTHONDONTWRITEBYTECODE=1` to prevent writing `.pyc` files to container filesystem layers.

### 8.14 DOC-014: Web Server Entrypoint Missing EXPOSE
- **Default Severity:** `INFO`
- **Description:** The container starts a web server but does not declare the listening network port via `EXPOSE`.
- **Remediation:** Add `EXPOSE <port>` (e.g. `EXPOSE 8000`) to document the port on which the container listens.

---

## 9. Engine & Execution Rules (`ENG-xxx`)

### 9.1 ENG-001: Analyzer Execution Failed
- **Default Severity:** `ERROR`
- **Description:** An analyzer raised an unexpected exception during execution.
- **Remediation:** Check the error message or report a bug to the analyzer maintainer.

---

## 10. Discovery & Parser Rules (`DISC-xxx`)

### 10.1 DISC-001: Unreadable Source File
- **Default Severity:** `WARNING`
- **Description:** A source file could not be read during project discovery due to permissions or encoding errors.
- **Remediation:** Check file read permissions and ensure file is encoded in valid UTF-8.

### 10.2 DISC-002: Python Syntax Error in Source File
- **Default Severity:** `WARNING`
- **Description:** A Python source file contains invalid syntax and could not be parsed into an AST.
- **Remediation:** Fix the syntax error in the source file.

---

## 11. CI/CD Workflow Rules (`CI-xxx`)

### 11.1 CI-001: CI Python Matrix Version Mismatch
- **Default Severity:** `WARNING`
- **Description:** CI workflow tests Python versions incompatible with declared project `requires-python` or classifiers in `pyproject.toml`.
- **Remediation:** Align CI test matrix versions with supported project Python ranges.

### 11.2 CI-002: Unfrozen Dependency Installation in CI
- **Default Severity:** `WARNING`
- **Description:** CI workflow installs dependencies without lockfiles or hash verification.
- **Remediation:** Use lockfiles (e.g. `uv sync --frozen`, `poetry install --sync`, or `pip-sync`) in CI workflows.

### 11.3 CI-003: Deprecated or Outdated CI Action Version
- **Default Severity:** `INFO`
- **Description:** Workflow references a deprecated or end-of-life GitHub Actions version (e.g. `actions/checkout@v2`, `actions/setup-python@v3`).
- **Remediation:** Upgrade GitHub Actions to current stable major releases (e.g. `actions/checkout@v4`, `actions/setup-python@v5`).

### 11.4 CI-004: Missing Test or Quality Gate in CI Workflow
- **Default Severity:** `INFO`
- **Description:** Pull request / push workflow contains no test suite, linter, or health checks.
- **Remediation:** Add automated test or lint steps (e.g. `pytest`, `ruff`, `stackvet scan --ci`).

### 11.5 CI-005: Insecure Secret or Token in CI Workflow
- **Default Severity:** `ERROR`
- **Description:** Potential plaintext secret or token literal detected in workflow file.
- **Remediation:** Store credentials in repository secrets (`${{ secrets.MY_SECRET }}`).

### 11.6 CI-006: Missing Concurrency Cancellation in PR Workflow
- **Default Severity:** `INFO`
- **Description:** Pull request workflow lacks concurrency cancellation, causing redundant runner minutes on repeated pushes.
- **Remediation:** Add concurrency group with `cancel-in-progress: true`.

---

## 12. Django Framework Rules (`DJG-xxx`)

### 12.1 DJG-001: Insecure Hardcoded DEBUG = True in Django Settings
- **Default Severity:** `ERROR`
- **Description:** Django settings hardcodes `DEBUG = True` without environment variable fallback, risking sensitive traceback exposure.
- **Remediation:** Load DEBUG dynamically from an environment variable: `DEBUG = os.getenv('DJANGO_DEBUG', 'False').lower() == 'true'`.

### 12.2 DJG-002: Hardcoded SECRET_KEY in Django Settings
- **Default Severity:** `ERROR`
- **Description:** Hardcoded SECRET_KEY literal detected in settings. Secrets must be loaded from environment variables or vault.
- **Remediation:** Load SECRET_KEY from environment: `SECRET_KEY = os.environ['DJANGO_SECRET_KEY']`.

### 12.3 DJG-003: Insecure Wildcard ALLOWED_HOSTS in Django Settings
- **Default Severity:** `WARNING`
- **Description:** `ALLOWED_HOSTS` contains wildcard `'*'` allowing Host header poisoning in production.
- **Remediation:** Explicitly define allowed domain names or load from environment.

### 12.4 DJG-004: Potential N+1 Database Query in Loop
- **Default Severity:** `WARNING`
- **Description:** QuerySet execution inside loop causes N+1 query overhead.
- **Remediation:** Prefetch related relations before looping: `queryset.prefetch_related('...')` or `select_related('...')`.

### 12.5 DJG-005: Django ForeignKey Missing Explicit on_delete
- **Default Severity:** `WARNING`
- **Description:** `models.ForeignKey` declared without required `on_delete` argument.
- **Remediation:** Specify explicit on_delete behavior: `models.CASCADE`, `models.PROTECT`, or `models.SET_NULL`.

### 12.6 DJG-006: Missing CSRF Protection Middleware
- **Default Severity:** `WARNING`
- **Description:** `MIDDLEWARE` list is missing `'django.middleware.csrf.CsrfViewMiddleware'`.
- **Remediation:** Add `'django.middleware.csrf.CsrfViewMiddleware'` to `MIDDLEWARE`.

---

## 13. Celery Framework Rules (`CEL-xxx`)

### 13.1 CEL-001: Insecure Celery Pickle Serializer Enabled
- **Default Severity:** `ERROR`
- **Description:** Celery serializer setting uses `'pickle'`, allowing arbitrary code execution if brokers are compromised.
- **Remediation:** Use secure serialization formats like `'json'` or `'msgpack'`.

### 13.2 CEL-002: Missing Task Timeout Limits in Celery Task
- **Default Severity:** `WARNING`
- **Description:** Celery task has no `time_limit` or `soft_time_limit` configured.
- **Remediation:** Specify `time_limit` and `soft_time_limit` on task decorator: `@shared_task(time_limit=300, soft_time_limit=240)`.

### 13.3 CEL-003: Unbounded Retry Policy on Celery Task
- **Default Severity:** `WARNING`
- **Description:** Celery task permits unlimited retries, risking broker congestion and infinite retry loops.
- **Remediation:** Set explicit `max_retries` (e.g. `max_retries=3, default_retry_delay=60`).

### 13.4 CEL-004: Blocking time.sleep() in Asynchronous Celery Task
- **Default Severity:** `WARNING`
- **Description:** Async Celery task invokes `time.sleep()`, blocking the async event loop.
- **Remediation:** Use `await asyncio.sleep()` instead of `time.sleep()`.

