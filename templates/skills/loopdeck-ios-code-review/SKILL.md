---
name: loopdeck:ios-code-review
description: This skill should be used when the user asks to review iOS code, audit a PR, check code quality, or verify that Swift/SwiftUI code conforms to project architecture. Validates MVVM + Interactor + Adapter layering, DI patterns, readability, test coverage, and alignment with the feature plan.
allowed-tools: [Read, Glob, Grep, Bash]
---

# iOS Code Reviewer

Review Swift/iOS code for architectural conformance, readability, test coverage, and plan alignment. This skill audits code — it does not apply fixes unless the user explicitly asks for them.

## Review Dimensions

Every review must cover these four dimensions:

### 1. Architecture Conformance (MVVM + Interactor + Adapter)

| Check | What to look for |
|-------|-----------------|
| Layer purity | View has no business logic; ViewModel has no UI framework imports beyond SwiftUI's `ObservableObject`; Interactor has no UI imports; Adapter is the only layer talking to external systems |
| Dependency direction | Dependencies flow inward: View → ViewModel → Interactor → Adapter. No reverse references |
| Protocol abstraction | Every class dependency is backed by a protocol. No concrete class is injected directly (except at the composition root) |
| Constructor injection | All dependencies passed via `init`, not resolved from a global container or singleton inside the class |
| ViewModel isolation | ViewModel does not own or reference SwiftUI `View` types, `@State`, `@Binding`, or `@Environment` |

Flag any violation with the file path and line, a severity (blocker / warning), and a suggested fix.

### 2. Readability & Code Quality

- **Naming**: Types and methods follow Swift API Design Guidelines. Clarity at the call site.
- **Single responsibility**: Each type has one clear job. ViewModels > 150 lines or Interactors with > 6 injected protocols are a smell.
- **Closure captures**: No unintentional `self` captures or retain cycles. Use `[weak self]` where the closure outlives the object.
- **Force unwrap**: No `!` outside tests. Use `guard let` or provide a default.
- **Code duplication**: Repeated logic across ViewModels or Interactors should be extracted into a shared helper or base protocol extension.
- **Access control**: Types and methods use the tightest access level needed (`private`, `fileprivate`, `internal`). Public API is intentional.

### 3. Test Coverage

| Check | What to look for |
|-------|-----------------|
| ViewModel tests exist | Every ViewModel has a corresponding `*ViewModelTests.swift` with at least one test per published method |
| Interactor tests exist | Every Interactor has corresponding tests. Adapter calls are mocked |
| Async coverage | Tests cover success, failure, and cancellation paths where applicable |
| Test isolation | Tests don't depend on shared mutable state. No test-order dependency |
| Mock quality | Mocks are simple and focused — avoid mock frameworks that obscure intent if hand-rolled stubs suffice |

Report coverage gaps: list which ViewModel or Interactor classes lack tests, and which methods within tested classes have no coverage.

### 4. Plan Alignment

When the user provides a feature plan, spec, or ticket reference (or a plan file is discoverable in the repo), verify:

- All planned UI states (loading, empty, error, success) are implemented in the View + ViewModel
- All planned interactions produce the expected Interactor calls
- Edge cases mentioned in the plan have corresponding handling
- No extra features beyond the plan scope (creep) without explicit justification

If no plan is available, note this and skip plan-alignment checks rather than guessing.

## Review Output Format

```markdown
## iOS Code Review — [Feature/Branch Name]

### Summary
- Files reviewed: N
- Blockers: X | Warnings: Y | Suggestions: Z
- Overall: ✅ Approve / ⚠️ Approve with comments / ❌ Request changes

### Blockers (must fix)
| # | File:Line | Issue | Fix |
|---|-----------|-------|-----|
| 1 | `Foo.swift:42` | ViewModel calls URLSession directly | Move to Adapter |

### Warnings (should fix)
| # | File:Line | Issue | Fix |
|---|-----------|-------|-----|
| 1 | `Bar.swift:18` | Force-unwrapped optional | Use `guard let` |

### Suggestions (nice to have)
- Consider extracting shared `loadData` logic into a protocol extension

### Test Coverage Gaps
- `CoffeeShopListViewModel`: missing test for error state
- `CoffeeDetailInteractor`: no test file found

### Architecture Diagram (if relevant)
(Include a quick ASCII diagram of the reviewed feature's dependency graph if it deviates from the expected pattern)
```

## When to Block

Flag as **blocker** (❌ Request changes):
- Layer violation: ViewModel directly calling network/db
- Missing protocol abstraction for injected dependency
- Retain cycle risk from strong `self` in escaping closures
- Force-unwrap in production code paths
- No tests at all for a new feature

## Before You Start

1. Ask the user for the feature plan or ticket if not provided and plan-alignment checks are needed
2. Identify all changed files with `git diff` if in a repo, or by scanning the feature directory
3. Read every changed file before reporting — do not review from diffs alone
