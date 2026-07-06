---
name: loopdeck:api-expert
description: This skill should be used when the user asks to "create an API contract", "design the API", "define endpoints", "write an OpenAPI spec", or needs a shared API contract between frontend and backend. Produces an OpenAPI 3.0 specification with request/response schemas, error formats, and examples. Must be invoked BEFORE any backend or frontend implementation begins.
allowed-tools: [Read, Write, Edit, Glob, Grep, Bash]
---

# API Expert — Design & Create API Contracts

Design the API contract that serves as the single source of truth for both the Go backend and iOS frontend. The contract must be complete, unambiguous, and locked before implementation starts.

## When This Skill Applies

- Before any backend or frontend code is written
- When a new feature requires new endpoints
- When the PRD describes data that flows between client and server
- The orchestrator invokes this as Phase 0 of any feature implementation

## Input

This skill receives context from the orchestrator (or user):
- Feature requirements from the PRD
- Clarified answers to ambiguous questions
- Data models and business rules

## Output

A complete API contract as `api/openapi.yaml` (OpenAPI 3.0). API design conventions are also documented at `docs/api/README.md`. The contract file must be checked into the repo alongside both stacks.

## API Design Principles

### RESTful Conventions

| Rule | Convention |
|------|-----------|
| Resource naming | Plural nouns: `/shops`, `/users`, `/orders` |
| Nested resources | `/shops/{shopId}/reviews` |
| HTTP verbs | GET (read), POST (create), PUT (full update), PATCH (partial update), DELETE (remove) |
| Idempotency | PUT and DELETE are idempotent; POST is not |
| Status codes | 200, 201, 204, 400, 401, 403, 404, 409, 422, 500 |
| Pagination | Cursor-based for large lists; `?cursor=xxx&limit=20` |
| Filtering | Query params: `?status=open&sort=rating:desc` |
| Versioning | Path-based: `/v1/shops`. Start with v1. |

### Request/Response Patterns

Every response follows a consistent envelope:

```yaml
# Success
{
  "data": { ... },           # The resource or list
  "meta": {                  # Optional metadata
    "cursor": "next-page-token",
    "count": 42
  }
}

# Error
{
  "error": {
    "code": "INVALID_INPUT",
    "message": "Human-readable description",
    "details": [             # Optional field-level errors
      { "field": "email", "reason": "already_taken" }
    ]
  }
}
```

### Schema Design Rules

- Use `camelCase` for all JSON keys
- Date/time as ISO 8601 strings (`2026-06-20T14:30:00Z`)
- IDs as strings (UUID) — not auto-increment integers
- Enums as uppercase strings: `"OPEN"`, `"CLOSED"`, `"TEMPORARILY_CLOSED"`
- Nullable fields marked explicitly with `nullable: true`
- Required arrays default to `[]`, not `null`

## Contract Structure

```yaml
openapi: "3.0.3"
info:
  title: "NgopiYuk API"
  description: "Backend API for the NgopiYuk coffee discovery app"
  version: "1.0.0"
servers:
  - url: "http://localhost:8080/v1"
    description: "Local development"
  - url: "https://api.ngopiyuk.app/v1"
    description: "Production"

paths:
  /shops:
    get:
      summary: "List nearby coffee shops"
      description: "Returns shops sorted by distance. Supports cursor pagination."
      parameters:
        - name: cursor
          in: query
          schema: { type: string }
        - name: limit
          in: query
          schema: { type: integer, default: 20, maximum: 100 }
        - name: lat
          in: query
          required: true
          schema: { type: number, format: double }
        - name: lng
          in: query
          required: true
          schema: { type: number, format: double }
      responses:
        "200":
          description: "Paginated list of shops"
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/ShopListResponse"
        "400":
          $ref: "#/components/responses/BadRequest"
        "401":
          $ref: "#/components/responses/Unauthorized"

components:
  schemas:
    Shop:
      type: object
      required: [id, name, latitude, longitude, rating]
      properties:
        id: { type: string, format: uuid, example: "550e8400-e29b-41d4-a716-446655440000" }
        name: { type: string, example: "Kopi Kenangan" }
        latitude: { type: number, format: double, example: -6.2088 }
        longitude: { type: number, format: double, example: 106.8456 }
        rating: { type: number, format: float, minimum: 0, maximum: 5, example: 4.5 }
        priceLevel: { type: integer, minimum: 1, maximum: 4, example: 2 }
        isOpen: { type: boolean, example: true }
        photoUrl: { type: string, format: uri, nullable: true }
        distance: { type: number, format: double, example: 350.0 }

    ErrorResponse:
      type: object
      required: [error]
      properties:
        error:
          type: object
          required: [code, message]
          properties:
            code: { type: string, example: "NOT_FOUND" }
            message: { type: string, example: "The requested shop was not found" }
            details:
              type: array
              items:
                type: object
                properties:
                  field: { type: string }
                  reason: { type: string }

  responses:
    BadRequest:
      description: "Invalid request"
      content:
        application/json:
          schema:
            $ref: "#/components/schemas/ErrorResponse"
    Unauthorized:
      description: "Missing or invalid authentication"
      content:
        application/json:
          schema:
            $ref: "#/components/schemas/ErrorResponse"
    NotFound:
      description: "Resource not found"
      content:
        application/json:
          schema:
            $ref: "#/components/schemas/ErrorResponse"

  securitySchemes:
    BearerAuth:
      type: http
      scheme: bearer
      bearerFormat: JWT

security:
  - BearerAuth: []
```

## Contract Checklist

Before finalizing the contract, verify:

- [ ] Every PRD user story maps to at least one endpoint
- [ ] All request/response schemas are defined (no `{}` placeholders)
- [ ] All error scenarios have a corresponding HTTP status code and error code
- [ ] Pagination is specified where lists may grow large
- [ ] Auth requirements are marked per-endpoint (public vs authenticated)
- [ ] Example values are provided for key fields
- [ ] Enums are explicitly listed (not just `type: string`)
- [ ] The contract file is saved to `api/openapi.yaml`
- [ ] Both the Go and iOS skill conventions are compatible with the design

## Red Flags to Flag to the User

- The PRD describes real-time features (chat, live location) but the contract is REST-only → suggest WebSocket/SSE
- File upload endpoints are described but no max-size or allowed-types are specified
- A single endpoint does too many things (list, filter, sort, aggregate in one call) → suggest splitting
- The PRD assumes the client can call external APIs directly (bypassing the backend) → flag security concern
- N+1 risk: an endpoint returns a list where each item requires a follow-up call for details → suggest embedding or a compound endpoint
