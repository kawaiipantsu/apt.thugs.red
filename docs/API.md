# Management API v1

The same management handlers serve the authorized Unix socket and authenticated
administrative HTTP. The public listener has no management routes. Socket group
membership grants full administrator authority; socket audit events use the
shared `local-socket` actor. HTTP events use the authenticated user's opaque ID.
The CLI currently uses the Unix transport only.

```sh
curl --unix-socket /run/xxc-aptd/admin.sock http://localhost/api/v1/status
curl --unix-socket /run/xxc-aptd/admin.sock -H 'Content-Type: application/octet-stream' \
  --data-binary @package.deb http://localhost/api/v1/uploads
```

See [openapi.json](openapi.json) for paths, authentication, request schemas and
structured errors, and [ADMINISTRATION.md](ADMINISTRATION.md) for roles and setup.

| Method | Path under /api/v1 | Response / minimum role |
| --- | --- | --- |
| GET | /status | Version, active count, staged count, generation; viewer |
| GET | /health | SQLite quick_check, directories, publication/signer configuration; viewer |
| GET | /packages, /uploads | Records; `q` FTS, zero-based `page`, 50 per page; viewer |
| GET | /packages/{id} | Inspected metadata; viewer |
| POST | /uploads | Raw streamed .deb; operator |
| POST | /uploads/{id}/stage | Stage an inspected record; operator |
| GET | /repository/diff | Review token, added/removed records, upgrades/older additions, architectures, size delta; viewer |
| GET | /repository/generations | Retained manifests; viewer |
| POST | /repository/publish | `{"review_token":"TOKEN_FROM_DIFF"}`; operator |
| POST | /repository/verify | Queue integrity/signature validation; operator |
| POST | /repository/reindex | Queue active-manifest reconciliation; operator |
| POST | /repository/rollback | `{"generation":"UUID"}`; operator |
| GET | /jobs, /jobs/{id} | Latest 100 jobs, or an ID from that window; viewer |
| GET | /audit | Latest 100 audit records, including actor/interface; viewer |
| GET | /config | Effective supported configuration; administrator |
| GET | /users | At most 1,000 safe user records, never password hashes; administrator |
| POST | /users | `username`, `password`, `role`; administrator |
| POST | /users/{id} | `action`: `password`, `role`, `enable`, `disable`, `delete`; administrator |

`password` changes additionally take `password`; `role` changes take `role`.
Roles are `viewer`, `operator`, `administrator`. The first user must be an
administrator and can only be bootstrapped through the socket. A user's password,
role or enabled-state change revokes all their sessions. The final enabled
administrator is protected.

## HTTP sessions

These endpoints exist only on the admin TCP listener:

1. `GET /api/v1/auth/challenge` sets a preauthentication cookie and returns `csrf`.
2. `POST /api/v1/auth/login` sends JSON `username`, `password`, `csrf`, the challenge
   cookie and an `Origin` matching `[admin].external_url`. On success it sets an
   HttpOnly session cookie and returns the safe user, session `csrf` and Unix
   expiry timestamp. The challenge is consumed once, even on incorrect credentials.
3. Send the session cookie on requests. Every mutation additionally needs the
   exact `Origin` and `X-CSRF-Token` containing the session CSRF token.
4. `GET /api/v1/auth/session` returns the current safe user, CSRF token and expiry.
5. `POST /api/v1/auth/logout` revokes that session and expires the cookie; CSRF and
   Origin checks still apply.

With an HTTPS `admin.external_url`, the session cookie is `__Host-xxc-session`
with Secure; explicit HTTP administration uses `xxc-session`. The public origin
and forwarding headers cannot change that choice.

The browser UI uses the same accounts and services. Project bearer tokens are
documented below; there are no CORS allowances or trusted identity headers. Login rate limits use the immediate
peer address. Do not put passwords or session credentials in URLs, command-line
arguments, logs or public examples. TLS remains the reverse proxy's responsibility.
Administrative `/healthz` and `/readyz` require a viewer session and currently
provide the same basic checks as `/api/v1/health`; disk/expiry readiness is pending.

## Publication and errors

Mutations use JSON except raw uploads. Publication jobs return 202 with `job_id`;
poll for `succeeded` or `failed`. Concurrent repository jobs return 409. Stage and
review operations share that lock. Publishing requires a token from a review of
the exact current selection; changed selections return 409
`publication_review_changed`. Upload concurrency is separately bounded. Transfer
limits return 413 (bytes) or 408 (deadline). Interrupted scratch files are removed.

Errors have `error.code`, a safe `error.message` and `error.request_id`. All API
responses use `Cache-Control: no-store` and `X-Request-ID`. An error header
matches its JSON request ID; accepted publication requests share the audit
request ID. Extractor errors use
the same envelope. Authentication failures use 401, role/Origin/CSRF failures 403,
login throttling 429, malformed requests/application validation 400 or 422, and
missing objects 404. Detailed failures remain in privileged logs.

Managed key APIs, package removal, GC, job cancellation, complete response schemas
for every legacy endpoint and remote CLI transport remain roadmap work.

## XXC Trust (administrator only)

All four endpoints use GET and the existing session or Unix socket authorization:

| Path | Response |
| --- | --- |
| `/api/v1/trust/status` | enabled, connected, inventory counts and configured selection availability |
| `/api/v1/trust/authorities` | items: ID, parent ID, display name, expiry, active flag |
| `/api/v1/trust/templates` | items: ID, name, lifetime, EKU, algorithm, domain suffix |
| `/api/v1/trust/certificates` | items with projected public certificate metadata, total, page, pages |

Certificate query parameters are `page` (1-based, max 1000000; zero means one),
`q` (at most 256 bytes) and `status` (active, expiring, expired, revoked or empty).
Unknown parameters fail. Configured authority selection filters the remote
query. Private material, owner IDs, CSRs and arbitrary upstream fields are never
returned. All reads audit the operation/result without inventory contents.

Disabled status returns `{"enabled":false,"connected":false}`; other disabled
operations return 503. Invalid filters return 400, bounded concurrency returns
503, and upstream failures return 502 with a safe `trust_*` code and request ID.
No upstream error body is forwarded. Public routes never expose this API.
See [XXC-TRUST.md](XXC-TRUST.md) and [OpenAPI](openapi.json).

## OpenPGP keys and remote signing

All `/api/v1/keys` operations require administrator authority over HTTP, or
filesystem authorization on the Unix socket. JSON errors and request IDs use
the common envelope. HTTP generation requires exact Origin and session CSRF.

| Method / path | Operation |
| --- | --- |
| GET `/api/v1/keys?page=1&q=` | Projected remote inventory; one-based pages, query up to 254 bytes |
| POST `/api/v1/keys` | Generate remotely, return 201 with public metadata |
| GET `/api/v1/keys/{id}` | Public metadata for a 32-character lowercase hex ID |
| GET `/api/v1/keys/{id}/public?format=binary` | Locally validated, normalized binary or armor public export |
| GET `/api/v1/keys/{id}/verify` | Check public bytes against metadata and active fingerprint pin when selected |

Generation requires `name`, `email`, `algorithm` (Ed25519, RSA-3072 or RSA-4096)
and `days` (1..3650). The name/email become part of the public key. Generation
is audited without storing identity fields in audit metadata. It is not retried
automatically and does not activate the key, publish it to CA exchange or rewrite
configuration. Owners, email/user-ID arrays and private fields are excluded from
metadata projections. Public key bytes inherently contain the chosen identity.

There is no private export/import, revoke, decrypt or arbitrary-payload signing
endpoint. Operators use the existing reviewed repository publish endpoint;
`signing.backend=xxc-trust` selects the remote signer. GPG verifies returned
artifacts and pinned fingerprints before publication. See [SIGNING.md](SIGNING.md).

## Automation, suites and analytics

The same API is available under `/admin/api/v1` on the private TCP listener for
path proxies. Original `/api/v1` paths and the Unix transport remain supported.
The public listener has no management API mount. See [AUTOMATION.md](AUTOMATION.md) for an
executable CI example and bearer token constraints.

| Endpoint | Behavior |
| --- | --- |
| GET /api/v1/suites | Configured suites (filtered to token permissions) and primary suite |
| GET /api/v1/tokens | Administrator-only metadata, never digests or secret values |
| POST /api/v1/tokens | Administrator-only `{name, scopes, suites, days}`; returns `{token, secret}` once |
| POST /api/v1/tokens/{id}/revoke | Administrator-only immediate revocation of new requests |
| GET /api/v1/analytics?days=30 | Viewer/session or Unix; 7/30/90-day private traffic aggregates |

Package list/show, uploads, stage, diff and publish accept `?suite=NAME`. Omission
uses the primary suite. The query parameter is the sole source of suite selection;
publish JSON remains `{review_token}`. A diff now includes `suite`. Unknown or
duplicate suite values fail. Review digests bind suite, selection, current generation
and signing/repository configuration. Whole-generation rollback affects every suite.
Tokens can see only jobs submitted with the same token identity, and cannot call
rollback, verification, reindex, config, audit, analytics, user or key routes.

Token permissions are `read`, `upload`, `stage`, `publish`; suites are an explicit
nonempty allowlist. Creation accepts 1–365 days. Auth uses Authorization: Bearer,
never URL parameters. Cookies alongside Authorization are rejected. Session auth
retains Origin/CSRF requirements; explicit bearer auth has no CSRF requirement.
Token responses carry no-store. Revocation cannot cancel already accepted jobs.


## Public documentation URLs

`GET /api` serves the developer guide, linked from public navigation and sitemap.
`/api/` redirects to `/api`. The same canonical automation text is downloadable
from `/api/guide.md`; `/api/openapi.json` serves this repository's OpenAPI file;
`/api/reference.md` serves the full canonical management reference. All accept
GET/HEAD without credentials, return no-cache and the usual security headers,
and contain no runtime admin origin, proxy addresses or credential values.
They cannot accept uploads or dispatch management requests.

The OpenAPI servers are public deployment examples. Integrators must confirm the
admin API base with the operator. Origin and CSRF parameters apply to session
authentication, not explicit bearer requests. Publishing response schemas cover
packages, suite lists, selection previews, accepted jobs, job state and errors.
The automation test executes the exact shell example downloaded from the guide
and verifies that its policy guard rejects unrelated staged additions.
