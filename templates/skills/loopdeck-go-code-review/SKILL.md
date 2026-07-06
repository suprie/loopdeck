---
name: loopdeck:go-code-review
description: This skill should be used when the user asks to review Go backend code, audit a Go PR, or check Go code quality. Validates Clean Architecture layering, interface-driven DI, error handling, test coverage, and alignment with the API contract in api/openapi.yaml.
allowed-tools: [Read, Glob, Grep, Bash]
---

# Go Code Reviewer

Review Go backend code for architectural conformance, readability, test coverage, and API contract alignment. This skill audits code — it does not apply fixes unless the user explicitly asks for them.

## Review Dimensions

### 1. Architecture Conformance (Clean Architecture)

| Check | What to look for |
|-------|-----------------|
| Layer purity | `handler/` has no DB or business logic; `usecase/` has no `net/http` or `database/sql` imports; `repository/` is the only layer with DB drivers |
| Dependency direction | handler → usecase → repository. No reverse imports. `domain/` imports nothing from `handler/`, `usecase/`, or `repository/` |
| Interface at layer boundary | Every usecase depends on a repository interface; every handler depends on a usecase interface. No concrete struct passed across layer boundaries |
| Constructor injection | All dependencies passed via `NewXxx(...)` constructors. No global singletons, no `init()` wiring |
| Main composition | `main.go` (or `cmd/`) is the only place that creates concrete implementations and wires them together |
| Package naming | No `utils`, `helpers`, `common` packages. Use purpose-named packages: `domain`, `handler`, `usecase`, `repository`, `config` |

Flag any violation with the file path, line, severity (blocker / warning), and a suggested fix.

### 2. Error Handling

| Check | What to look for |
|-------|-----------------|
| No ignored errors | `_` assigned to an error return is flagged. Inline `if err != nil { return ... }` is the expected pattern |
| Error wrapping | Errors are wrapped with `fmt.Errorf("context: %w", err)` at each layer boundary. Use `%w`, not `%v` |
| Domain errors | `domain/errors.go` (or similar) defines sentinel errors (`var ErrNotFound = ...`). Layers use `errors.Is()` and `errors.As()` |
| HTTP error mapping | The handler translates Go errors to correct HTTP status codes (e.g., `ErrNotFound` → 404). No raw error strings leaked to HTTP responses |
| No panic | `panic()` only in truly unrecoverable situations (config missing at startup). Never in request-handling code |

### 3. Readability & Idiomatic Go

- **Naming**: Interfaces are single-method where possible (`ShopReader`, `ShopWriter`). Multi-method interfaces are named `XxxRepository` or `XxxService`. Method names follow Go conventions (no `Get` prefix unless needed).
- **Package size**: Packages under 500 lines. A `usecase/` package with every usecase in one file is fine if small; split by domain entity otherwise.
- **Exported vs unexported**: Only types and functions used outside the package are exported (capitalized). Test files may use `export_test.go` pattern for white-box tests.
- **Context propagation**: Every function that crosses a layer boundary takes `ctx context.Context` as the first parameter. Context is not stored in structs.
- **Goroutine safety**: Shared mutable state is behind a mutex or uses channels. Repository implementations that use `*sql.DB` (which is pool-safe) are fine.
- **Defer cleanup**: Resources (files, response bodies, rows) are closed with `defer` immediately after acquisition.
- **Magic numbers**: Named constants, not bare ints/strings. HTTP status codes use `net/http` constants (`http.StatusOK`, not `200`).

### 4. Test Coverage

| Check | What to look for |
|-------|-----------------|
| Usecase tests | Every usecase method has table-driven tests covering success, not-found, and error paths |
| Handler tests | At minimum, happy-path tests with `httptest.NewRecorder()`. Error-mapping tests for critical endpoints |
| Repository tests | If present: integration tests. If absent: note the gap, but don't block unless CI requires them |
| Mock quality | Mocks are generated with `testify/mock` or hand-rolled. Mocks are in `_test.go` files or a `mocks/` subpackage |
| Table-driven | Tests use the table-driven pattern. Each case has a name, inputs, mock setup, expected output, and expected error |

### 5. API Contract Alignment

Read `api/openapi.yaml` and verify:
- Every endpoint in the contract has a corresponding handler registration in the router
- Handler response structs match the contract's response schemas (field names, types, required fields)
- Error response format matches the contract's `ErrorResponse` schema
- Query parameter names match between contract and handler parsing
- Auth middleware is applied to endpoints marked with `security` in the contract

### 6. Security

| Check | What to look for |
|-------|-----------------|
| Input validation | All user input is validated. Use struct tags with `validator` library or explicit checks |
| SQL injection | All DB queries use parameterized arguments (`$1`, `$2`). No string concatenation for query building |
| Auth middleware | Authenticated endpoints have middleware that validates JWT/ session before the handler runs |
| Sensitive data | No secrets, passwords, or tokens logged. `slog` or `log` calls with user data use redaction |

## Review Output Format

```markdown
## Go Code Review — [Feature/Branch Name]

### Summary
- Files reviewed: N
- Blockers: X | Warnings: Y | Suggestions: Z
- Overall: ✅ Approve / ⚠️ Approve with comments / ❌ Request changes

### Blockers (must fix)
| # | File:Line | Issue | Fix |
|---|-----------|-------|-----|
| 1 | `handler/shop.go:42` | Handler calls `db.Query` directly | Move to repository → usecase |

### Warnings (should fix)
| # | File:Line | Issue | Fix |
|---|-----------|-------|-----|
| 1 | `usecase/shop.go:28` | Error not wrapped with context | Use `fmt.Errorf("list shops: %w", err)` |

### Suggestions (nice to have)
- Consider splitting `usecase/shop.go` (380 lines) into reader and writer interfaces

### API Contract Gaps
- `GET /v1/shops/{id}/reviews` is in the contract but has no handler registered
- `ShopListResponse.meta.count` is in the contract but missing from the handler's response struct

### Test Coverage Gaps
- `handler/shop_handler.go`: no tests
- `usecase/shop_usecase.go` `UpdateShop`: no error-path test
```

## When to Block

Flag as **blocker** (❌ Request changes):
- Layer violation: handler directly calling DB; usecase importing `net/http`
- Missing interface at a layer boundary (concrete struct injected instead of interface)
- Ignored error return (assigned to `_`)
- Panic in request-handling code path
- SQL string concatenation (injection risk)
- Missing auth on an endpoint the contract marks as secured
- No tests at all for a new feature

## Before You Start

1. Identify all changed Go files (`git diff` or scan `server/`)
2. Read `api/openapi.yaml` to understand the expected contract
3. Read every changed file before reporting — do not review from diffs alone
4. If no API contract file exists, note it and skip contract-alignment checks
