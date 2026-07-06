---
name: loopdeck:go-dev
description: This skill should be used when the user asks to create Go backend code, implement a Go service, build an API server, or write Go handlers/services/repositories. Follows Clean Architecture with handler → usecase → repository layers, interface-driven DI, and table-driven tests. Builds against the API contract in api/openapi.yaml.
allowed-tools: [Read, Write, Edit, Glob, Grep, Bash]
---

# Go Developer — Clean Architecture Backend

Build Go backend services following Clean Architecture layers with strict separation of concerns. All code is driven by the API contract in `api/openapi.yaml`.

## Architecture Overview

```
┌──────────────────────────────────────────────┐
│  Handler (HTTP layer)                         │
│  - Parses request, validates input            │
│  - Calls usecase with domain types            │
│  - Serializes response, maps errors to HTTP   │
│  - No business logic, no direct DB calls      │
│  - Depends on: Usecase interface              │
└──────────────┬───────────────────────────────┘
               │
┌──────────────▼───────────────────────────────┐
│  Usecase (business logic layer)               │
│  - Contains all business rules                │
│  - Orchestrates repository calls              │
│  - Pure Go, no HTTP or DB imports             │
│  - Depends on: Repository interfaces          │
└──────────────┬───────────────────────────────┘
               │
┌──────────────▼───────────────────────────────┐
│  Repository (data access layer)               │
│  - Database queries, external API calls       │
│  - Returns domain models, not DB rows         │
│  - Depends on: *sql.DB, *redis.Client, etc.   │
└──────────────────────────────────────────────┘
```

### Dependency Direction

```
handler → usecase → repository
   ↓         ↓           ↓
  (all dependencies point inward via interfaces)
```

- Handler knows about usecase interfaces
- Usecase knows about repository interfaces
- Nothing in `usecase/` imports `net/http` or `database/sql`
- Nothing in `handler/` imports `database/sql`

## Project Structure

```
server/
├── main.go                    # Entry point, wires DI
├── go.mod
├── go.sum
├── cmd/
│   └── server/
│       └── main.go            # Alternative: cmd-level entry
├── internal/
│   ├── handler/               # HTTP handlers
│   │   ├── shop_handler.go
│   │   ├── auth_handler.go
│   │   └── middleware/
│   │       └── auth.go
│   ├── usecase/               # Business logic
│   │   ├── shop_usecase.go
│   │   ├── shop_usecase_test.go
│   │   ├── auth_usecase.go
│   │   └── auth_usecase_test.go
│   ├── repository/            # Data access
│   │   ├── shop_repo.go
│   │   ├── shop_repo_postgres.go
│   │   ├── user_repo.go
│   │   └── user_repo_postgres.go
│   ├── domain/                # Shared domain models
│   │   ├── shop.go
│   │   ├── user.go
│   │   └── errors.go
│   └── config/
│       └── config.go
├── migrations/                # SQL migration files
│   ├── 001_create_users.up.sql
│   └── 001_create_users.down.sql
├── api/
│   └── openapi.yaml           # API contract (shared with iOS)
└── Makefile
```

## Code Conventions

### Interfaces

Every layer depends on interfaces, defined alongside the consumer (not the implementation):

```go
// In usecase/shop_usecase.go — the consumer defines the interface it needs
type ShopRepository interface {
    ListShops(ctx context.Context, params ListShopsParams) ([]domain.Shop, string, error)
    GetShopByID(ctx context.Context, id string) (domain.Shop, error)
    CreateShop(ctx context.Context, shop domain.Shop) (domain.Shop, error)
}

type ShopUsecase struct {
    repo ShopRepository
}

func NewShopUsecase(repo ShopRepository) *ShopUsecase {
    return &ShopUsecase{repo: repo}
}
```

### Error Handling

```go
// Define domain errors in domain/errors.go
var (
    ErrNotFound     = errors.New("resource not found")
    ErrUnauthorized = errors.New("unauthorized")
    ErrConflict     = errors.New("resource already exists")
)

// Wrap with context at each layer
func (uc *ShopUsecase) GetShop(ctx context.Context, id string) (domain.Shop, error) {
    shop, err := uc.repo.GetShopByID(ctx, id)
    if err != nil {
        if errors.Is(err, ErrNotFound) {
            return domain.Shop{}, fmt.Errorf("shop %s: %w", id, ErrNotFound)
        }
        return domain.Shop{}, fmt.Errorf("get shop %s: %w", id, err)
    }
    return shop, nil
}
```

### Handler Pattern

```go
func (h *ShopHandler) ListShops(w http.ResponseWriter, r *http.Request) {
    // 1. Parse and validate input
    params, err := parseListShopsParams(r)
    if err != nil {
        writeError(w, r, err)
        return
    }

    // 2. Call usecase
    shops, cursor, err := h.usecase.ListShops(r.Context(), params)
    if err != nil {
        writeError(w, r, err)
        return
    }

    // 3. Write response
    writeJSON(w, http.StatusOK, ListShopsResponse{
        Data: shops,
        Meta: Meta{Cursor: cursor, Count: len(shops)},
    })
}
```

### Constructor Injection (main.go wiring)

```go
func main() {
    cfg := config.Load()
    db := mustConnectDB(cfg.DatabaseURL)

    // Repository layer
    shopRepo := repository.NewShopRepoPostgres(db)
    userRepo := repository.NewUserRepoPostgres(db)

    // Usecase layer
    shopUC := usecase.NewShopUsecase(shopRepo)
    authUC := usecase.NewAuthUsecase(userRepo, cfg.JWTSecret)

    // Handler layer
    shopHandler := handler.NewShopHandler(shopUC)
    authHandler := handler.NewAuthHandler(authUC)

    // Router
    r := chi.NewRouter()
    r.Use(middleware.Logger)
    r.Get("/v1/shops", shopHandler.ListShops)
    r.Post("/v1/auth/login", authHandler.Login)

    http.ListenAndServe(":8080", r)
}
```

## Testing

### Table-Driven Tests

```go
func TestListShops(t *testing.T) {
    tests := []struct {
        name    string
        params  ListShopsParams
        mockFn  func(*MockShopRepo)
        want    []domain.Shop
        wantErr bool
    }{
        {
            name:   "success with results",
            params: ListShopsParams{Lat: -6.2, Lng: 106.8, Limit: 20},
            mockFn: func(m *MockShopRepo) {
                m.On("ListShops", mock.Anything, mock.Anything).
                    Return([]domain.Shop{{ID: "1", Name: "Test"}}, "next-cursor", nil)
            },
            want:    []domain.Shop{{ID: "1", Name: "Test"}},
            wantErr: false,
        },
        {
            name:   "repository error",
            params: ListShopsParams{Lat: -6.2, Lng: 106.8, Limit: 20},
            mockFn: func(m *MockShopRepo) {
                m.On("ListShops", mock.Anything, mock.Anything).
                    Return(nil, "", errors.New("db down"))
            },
            wantErr: true,
        },
    }

    for _, tt := range tests {
        t.Run(tt.name, func(t *testing.T) {
            mockRepo := new(MockShopRepo)
            tt.mockFn(mockRepo)
            uc := NewShopUsecase(mockRepo)

            got, _, err := uc.ListShops(context.Background(), tt.params)

            if tt.wantErr {
                require.Error(t, err)
                return
            }
            require.NoError(t, err)
            assert.Equal(t, tt.want, got)
        })
    }
}
```

### What to Test

- **Usecase layer**: Every method. Mock the repository interface. Test success, not-found, and error paths.
- **Handler layer**: Test request parsing and error mapping with `httptest.NewRecorder()`. Mock the usecase.
- **Repository layer**: Integration tests against a test database (or skip if CI doesn't have one — document the gap).

### Test File Convention

- Test file alongside source: `shop_usecase.go` → `shop_usecase_test.go`
- Use `testify` for assertions (`assert`, `require`) and mocks (`mock.Mock`)
- Use `go test -v -cover ./...` to run all tests

## Build & Run

```bash
# Development
cd server && go run ./cmd/server/

# Test
go test ./... -v -cover

# Lint (requires golangci-lint)
golangci-lint run ./...

# Build
go build -o bin/server ./cmd/server/

# Migration (requires golang-migrate)
migrate -path migrations -database "$DATABASE_URL" up
```

## Package Choice Guidance

| Concern | Package |
|---------|---------|
| HTTP router | `chi` (lightweight, stdlib-compatible) |
| Validation | `go-playground/validator` |
| Database | `pgx` (PostgreSQL) or `sqlite3` for local dev |
| Migrations | `golang-migrate/migrate` |
| Logging | `slog` (stdlib, Go 1.21+) |
| Config | `envconfig` or `caarlos0/env` |
| Testing | `testify/assert`, `testify/require`, `testify/mock` |
| JWT | `golang-jwt/jwt/v5` |
