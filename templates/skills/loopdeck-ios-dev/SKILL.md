---
name: loopdeck:ios-dev
description: This skill should be used when the user asks to create iOS UI, build SwiftUI views, implement a screen or feature for iOS, or write Swift code following the project architecture. Covers MVVM + Interactor + Adapter, dependency injection, testable design, and unit tests.
allowed-tools: [Read, Write, Edit, Glob, Grep, Bash]
---

# iOS Developer — SwiftUI + MVVM + Interactor + Adapter

Create and maintain iOS features following this project's layered architecture. Every screen, component, or feature must adhere to these patterns.

## 🔴 HARD REQUIREMENT: XcodeGen Only

**Never edit `*.xcodeproj/project.pbxproj` directly.** All source files under `NgopiYuk/` are auto-discovered by xcodegen via the `project.yml` `sources` glob. To add files:

1. Create the `.swift` file in the correct folder under `NgopiYuk/`
2. Run `xcodegen generate` to regenerate the project

Do NOT use `plutil`, manual pbxproj editing, or any Xcode project manipulation tool. xcodegen is the single source of truth for project structure.

## 🔴 HARD REQUIREMENT: Modular Architecture (ADR-001)

**Every tab/feature is its own module. The main app depends ONLY on module API protocols — never on concrete module implementations.**

Read `docs/decisions/ADR-001-modular-architecture.md` for the full rationale. Violating these rules is a BLOCKER in code review.

### Module Rules (non-negotiable)

1. **Each module has exactly ONE public protocol** in an `API/` subfolder (e.g., `TodayModuleAPI.swift`)
2. **All module internals are `internal` or `private`** — Views, ViewModels, Interactors are never `public`
3. **Module X must NEVER import Module Y** — modules are independent
4. **Module may ONLY depend on:** `Core/Models/`, `Core/DesignSystem/`, `Core/Components/`, `Core/Services/`
5. **Module View factory returns `AnyView`** — `func makeXView() -> AnyView` — type erasure at the API boundary is acceptable for tab-level views
6. **Module init takes a shared dependency bag** (protocol or concrete `CoreServices`)

### Module File Structure (per module)

```
Modules/[ModuleName]Module/
├── API/
│   └── [ModuleName]ModuleAPI.swift    # The ONLY public protocol
├── [ModuleName]View.swift             # internal — SwiftUI View
├── [ModuleName]ViewModel.swift        # internal — ObservableObject
└── [ModuleName]Interactor.swift       # internal — business logic (if needed)
```

### Module API Template

```swift
// Modules/TodayModule/API/TodayModuleAPI.swift
import SwiftUI

protocol TodayModuleAPI {
    func makeTodayView() -> AnyView
}
```

### Module Implementation Template

```swift
// Modules/TodayModule/TodayModule.swift
import SwiftUI
import Core.Models
import Core.DesignSystem
import Core.Services

final class TodayModule: TodayModuleAPI {
    private let services: CoreServicesProtocol

    init(services: CoreServicesProtocol) {
        self.services = services
    }

    func makeTodayView() -> AnyView {
        let vm = TodayViewModel(services: services)
        return AnyView(TodayView(viewModel: vm))
    }
}
```

### Main App Wiring (in AppContainer)

```swift
// App/AppContainer.swift
@MainActor
final class AppContainer: ObservableObject {
    let todayModule: TodayModuleAPI
    let beansModule: BeansModuleAPI
    // ... one property per module, typed by protocol only

    init() {
        let services = CoreServices(modelContainer: ...)
        self.todayModule = TodayModule(services: services)
        self.beansModule = BeansModule(services: services)
        // ...
    }
}
```

## Architecture Overview

```
┌─────────────────────────────────────────┐
│  View (SwiftUI)                          │
│  - Owns a @StateObject / @ObservedObject │
│  - Renders ViewModel state               │
│  - Sends user intent to ViewModel        │
│  - Zero business logic                   │
└──────────────┬──────────────────────────┘
               │ @Published properties
               │ user intent methods
┌──────────────▼──────────────────────────┐
│  ViewModel (ObservableObject)            │
│  - Publishes UI state via @Published     │
│  - Delegates business logic to Interactor│
│  - Formats/transforms data for the View  │
│  - No UIKit, no SwiftUI imports (except  │
│    ObservableObject when needed)         │
│  - Injected: Interactor via protocol     │
└──────────────┬──────────────────────────┘
               │ async / Combine
┌──────────────▼──────────────────────────┐
│  Interactor (protocol-driven)            │
│  - Contains all business logic           │
│  - Orchestrates Adapter calls            │
│  - Stateless where possible; pure logic  │
│  - No UI framework imports               │
│  - Injected: Adapter protocols           │
└──────────────┬──────────────────────────┘
               │ async / Combine
┌──────────────▼──────────────────────────┐
│  Adapter (protocol-driven)               │
│  - Talks to the outside world            │
│  - Network, database, UserDefaults, etc. │
│  - Returns domain models, not DTOs       │
│  - Injected via protocol into Interactor │
└─────────────────────────────────────────┘
```

## Dependency Injection

Use a lightweight DI container or manual constructor injection. Prefer constructor injection — every dependency is a protocol, passed at init time.

```swift
// Protocol for every dependency
protocol CoffeeShopServiceProtocol {
    func fetchNearbyShops() async throws -> [CoffeeShop]
}

// Adapter conforms to the protocol
final class CoffeeShopService: CoffeeShopServiceProtocol { ... }

// Interactor depends on the protocol
final class CoffeeShopInteractor: CoffeeShopInteractorProtocol {
    private let service: CoffeeShopServiceProtocol
    init(service: CoffeeShopServiceProtocol) { self.service = service }
}

// ViewModel depends on the Interactor protocol
final class CoffeeShopListViewModel: ObservableObject {
    private let interactor: CoffeeShopInteractorProtocol
    init(interactor: CoffeeShopInteractorProtocol) { self.interactor = interactor }
}
```

## File Naming & Structure

For a feature named `CoffeeShopList`:

```
Features/CoffeeShopList/
├── CoffeeShopListView.swift        # SwiftUI View
├── CoffeeShopListViewModel.swift   # ObservableObject ViewModel
├── CoffeeShopListInteractor.swift  # Protocol + implementation
├── CoffeeShopListInteractorTests.swift
├── CoffeeShopListViewModelTests.swift
├── CoffeeShopAdapter.swift         # Protocol + implementation (if feature-specific)
└── Models/
    └── CoffeeShop.swift            # Domain model
```

Shared adapters/services go in `Core/Services/`.

## Code Style Rules

- **Views**: Structs, no `@State` for business data — only for local UI state (sheet toggles, etc.). Always use `@StateObject` for the ViewModel.
- **ViewModels**: `final class`, `ObservableObject`, `@Published` for all UI-bound state. No network/db calls directly — always through the Interactor protocol.
- **Interactor**: `final class` implementing a protocol. Takes Adapter protocols in `init`. Methods are `func ...() async throws` or use Combine publishers. Never references `@Published` or UI types.
- **Adapters**: `final class` implementing a protocol. Maps DTOs → domain models. All external calls isolated here.
- **Protocols**: Every class dependency is backed by a protocol. Name them `XxxProtocol` or `XxxUseCase` consistently within the project.
- **Testing**: Mock every protocol for unit tests. ViewModel tests inject a mock Interactor. Interactor tests inject a mock Adapter.

## When Creating a New Feature

1. Define the domain models first (`Models/`)
2. Write the Adapter protocol and a stub implementation
3. Write the Interactor protocol + implementation, injecting the Adapter
4. Write unit tests for the Interactor (mock the Adapter)
5. Write the ViewModel, injecting the Interactor — publish state, expose intent methods
6. Write unit tests for the ViewModel (mock the Interactor)
7. Write the SwiftUI View, wiring it to the ViewModel
8. Write snapshot or UI tests for the View if applicable

## Unit Test Requirements

- Every ViewModel must have a corresponding test class
- Every Interactor must have a corresponding test class
- Use `XCTest` + async/await (`XCTestExpectation` or `await`)
- Mock protocols with simple in-memory stubs or a library (e.g. `Mockingbird`, `Cuckoo`, or hand-rolled)
- Test happy path, errors, and edge cases
- Name tests: `test_<method>_<scenario>_<expectedResult>()`

```swift
func test_fetchShops_success_updatesShops() async throws {
    let mockService = MockCoffeeShopService(shops: [.stub()])
    let interactor = CoffeeShopInteractor(service: mockService)
    let viewModel = CoffeeShopListViewModel(interactor: interactor)

    await viewModel.loadShops()

    XCTAssertFalse(viewModel.shops.isEmpty)
}
```
