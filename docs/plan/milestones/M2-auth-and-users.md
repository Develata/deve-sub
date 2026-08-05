# Milestone 2 — Auth and Users

## Scope

Authentication, authorization, session management, two-factor authentication,
and user management. M2 delivers the identity layer: admin initialization,
login/logout with secure sessions, RBAC guards, 2FA (TOTP + recovery codes),
login rate limiting, CSRF protection, and user lifecycle (enable, disable,
force logout).

## Dependency

M1 (Infrastructure) must be complete. The Axum server, SQLite pool, migration
0002 (users, sessions, audit_log, outbox_event), CLI framework, and OpenAPI
toolchain are prerequisites.

## Vertical slice

```text
deve-sub user init-admin --username admin --password ...
    → first admin user created (only once)
deve-sub serve
    → POST /api/v1/auth/login { username, password }
        → validates credentials, creates session, sets HttpOnly cookie
        → returns user DTO with role
    → GET /api/v1/auth/me (with session cookie)
        → returns current user
    → POST /api/v1/auth/logout (with session cookie)
        → revokes session, clears cookie
    → POST /api/v1/users (admin only)
        → creates new user
    → GET /api/v1/users (admin only)
        → lists users with cursor pagination
    → POST /api/v1/users/{id}/disable (admin only)
        → disables user, revokes all sessions
    → POST /api/v1/auth/2fa/setup (authenticated)
        → returns TOTP secret and QR URI
    → POST /api/v1/auth/2fa/verify (authenticated)
        → verifies TOTP code, enables 2FA, returns recovery codes
    → POST /api/v1/auth/login with 2FA
        → returns 2FA challenge
    → POST /api/v1/auth/login/2fa { code }
        → completes login
```

## Deliverables

- Security crate: argon2id password hashing, CSPRNG session token generation,
  HMAC-SHA256 token hashing, TOTP (RFC 6238), recovery code generation.
- Domain identity module: `User` aggregate, `Session` entity, `Role` enum,
  `UserRepository` and `SessionRepository` port traits.
- Application auth module: `setup_admin`, `login`, `logout`, `disable_user`,
  `force_logout`, `setup_2fa`, `verify_2fa`, `disable_2fa`,
  `regenerate_recovery_codes` commands; `list_users`, `get_user`,
  `get_current_user` queries.
- Contract auth DTOs: login request/response, setup-admin request, user DTO,
  2FA DTOs, error response.
- Storage adapter: `SqliteUserRepository`, `SqliteSessionRepository`.
- Server: auth routes (`/api/v1/auth/*`), user management routes
  (`/api/v1/users/*`), `AuthUser` extractor, `AdminUser` guard, session
  cookie handling.
- CLI: `deve-sub user init-admin` subcommand.
- Migration 0003: `totp_secret` and `recovery_code` tables, users table
  additions for `last_login_at`. Rate limiting is in-memory (no migration
  needed) — see the module doc on `deve-sub-inmemory::rate_limiter`.
- Config: `SecurityConfig` with master key path for HMAC and encryption.
- Session tokens: CSPRNG-generated, stored as HMAC-SHA256 digests, sent as
  `SameSite=Lax` `HttpOnly` cookies. Tokens redacted in logs (SEC-009).
- Login rate limiting: per-username and per-IP tracking, temporary lockout
  after threshold failures (AUTH-004).
- CSRF protection: `SameSite=Lax` cookies plus `Origin` header validation for
  state-changing requests (SEC-010).

## Slicing

M2 is delivered in four slices:

1. **Auth foundation**: security primitives, domain identity, application
   commands (setup-admin, login, logout), storage adapter, server routes,
   CLI `user init-admin`. Acceptance: AUTH-001, AUTH-002, AUTH-003, SEC-009.
2. **RBAC and user management**: `AdminUser` guard, user CRUD, disable user,
   force logout. Acceptance: AUTH-007, AUTH-008, AUTH-010.
3. **Rate limiting and CSRF**: login rate limiting, CSRF protection.
   Acceptance: AUTH-004, SEC-010.
4. **2FA**: migration 0003, TOTP setup/verify/disable, recovery codes, 2FA
   login flow. Acceptance: AUTH-005, AUTH-006.

AUTH-009 (Token 重置 — "旧订阅 Token 失效") depends on subscription tokens
(M6) and remains `planned` through M2.

## Authority

- Architecture: `docs/plan/03-architecture.md`
- Storage: `docs/plan/13-storage.md`, ADR-0002
- API/OpenAPI: ADR-0004
- Security: `docs/plan/00-engineering-constitution.md` §"Data and security"
- Entity model: `docs/data-model/core-er.md`, `docs/data-model/entity-catalog.md`

## Verification

- `deve-sub user init-admin` creates the first admin and refuses a second.
- `POST /api/v1/auth/login` with correct credentials returns a session cookie.
- `POST /api/v1/auth/login` with wrong credentials returns 401 without
  leaking whether the username exists.
- `GET /api/v1/auth/me` with a valid session returns the user DTO.
- `POST /api/v1/auth/logout` revokes the session and clears the cookie.
- Admin-only routes return 403 for regular users.
- Disabling a user revokes all their sessions.
- Force logout revokes a specified session.
- Login rate limiting temporarily locks after repeated failures.
- 2FA setup, verify, and login flow completes end-to-end.
- Recovery codes are single-use.
- Session tokens are redacted in logs.
- Cross-site write requests are rejected by CSRF protection.
- Acceptance: `AUTH-001` through `AUTH-008`, `AUTH-010`, `SEC-009`, `SEC-010`.

## Deferred items

The following items are acknowledged limitations from M2, registered for
later-milestone action:

- **Cross-repository 2FA orchestration (D3)**: 2FA enable/disable spans the
  user repository and the 2FA repository (TOTP secrets + recovery codes)
  without a cross-repository transaction. Reliability is maintained via
  compensating actions (eager delete on failure, tolerant queries that only
  read when `two_factor_enabled` is set) and documented with WHY comments.
  This is an architectural constraint of the modular monolith; deferred to a
  future transactional outbox or saga pattern if reliability requirements
  escalate.

- **Application-layer `is_active` guard (D4)**: 2FA commands
  (`setup_2fa`, `disable_2fa`, `regenerate_recovery_codes`) rely on the
  delivery-layer `AdminUser` guard for authorization and do not re-check
  `user.is_active()` in the application layer. Defense-in-depth would add
  this check; deferred until the command layer gains a uniform guard policy.

- **Master key rotation (D5)**: The master key has no version or rotation
  mechanism. A single 32-byte key serves session token HMAC, recovery code
  HMAC, and TOTP/field encryption (XChaCha20-Poly1305). Rotating the key
  would invalidate all sessions, recovery codes, and encrypted TOTP secrets
  simultaneously with no key-ID decoupling. Deferred to M8 (Deployment and
  Hardening) when key management and rotation are formally specified.
