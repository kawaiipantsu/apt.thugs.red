# Project publishing API

Build a client that uploads Debian packages, stages them in a suite, reviews the
changes and publishes a signed archive generation. This guide is available as
HTML at `/api`, plain Markdown at `/api/guide.md`, and an OpenAPI reference at
`/api/openapi.json`. Share `/api` with implementation developers or coding agents.

## API base and requirements

Get the administrative API base URL and a project token from the repository
operator. For a same-host path proxy the base is
`https://apt.thugs.red/admin/api/v1`. A dedicated admin host may instead use
`https://admin.example.invalid/api/v1`. These are deployment examples: the
operator must enable the admin proxy route before it can accept requests.
The documentation URL is not the upload URL. The public listener serves this
guide but has no management handlers, including at `/api/v1`.

Use a server-side integration with TLS verification enabled. The example below
needs a POSIX shell, curl, Python 3 and jq, a built `.deb`, and a CI file secret.
Use the administrative API's configured Host; do not guess internal listener
addresses or turn off authentication, CSRF, TLS verification or proxy controls.
The daemon itself speaks HTTP behind its TLS reverse proxy.

## Create a credential

Sign in as an administrator and open **API tokens**. Give the token a project
name, explicit suites, permissions and a lifetime of 1–365 days (default UI: 90).
Copy the one-time value into the project's protected CI secret store. It is never
shown again. SQLite stores only its SHA-256 digest. List metadata includes its ID,
permissions, expiry, last use (updated at most once a minute), and revocation.
Revoke from the same page; create a replacement for rotation. Revocation rejects
new requests immediately; already authorized work may complete.

| Permission | Allowed operation |
| --- | --- |
| read | Status, permitted suite/package metadata, review diff and this token's jobs |
| upload | Stream a `.deb` into a permitted suite's quarantine membership |
| stage | Stage an uploaded package in that suite |
| publish | Publish that suite using a fresh review token |

Tokens cannot sign arbitrary bytes, administer users/keys/tokens, read config,
audit or analytics, roll back the archive, or log in to HTML pages. Grant only
needed permissions. No wildcard suite permission exists; adding a suite does not
expand old tokens. Omitted `suite` means the configured default, which must also
be authorized. Unknown suites and duplicate suite query parameters are rejected.

Send `Authorization: Bearer VALUE`. A bearer request must not carry cookies.
Bearer auth is explicit and does not require CSRF; browser session mutations still
require exact Origin and CSRF. There is no CORS bypass. Never put tokens in query
strings, command arguments, Git, package metadata or build logs.

## Endpoint reference

Paths below are relative to the API base, including `/api/v1`. Append
`?suite=nightly` to every project request, including status and job polling, when
that is the token's authorized suite. Suite names are case-sensitive. Ask the
operator for an allowed suite before calling `/suites`; omitting it selects the
default suite, which the token might not be allowed to read.

| Method | Path | Permission | Success |
| --- | --- | --- | --- |
| GET | `/status` | read | 200: version, package/staged counts and generation |
| GET | `/suites` | read | 200: default and permitted suite names |
| GET | `/packages` or `/uploads` | read | 200: packages array; `q` searches, `page` starts at 0, 50 records per page |
| GET | `/packages/{id}` | read | 200: inspected package metadata within the selected suite |
| POST | `/uploads` | upload | 200: package metadata including the ID; raw `.deb` body |
| POST | `/uploads/{id}/stage` | stage | 200: staged true; no request body needed |
| GET | `/repository/diff` | read | 200: suite, review token, added/removed records and version/size changes |
| POST | `/repository/publish` | publish | 202: job_id; JSON body contains review_token |
| GET | `/jobs` | read | 200: this token's jobs within the most recent 100 archive jobs |
| GET | `/jobs/{id}` | read | 200: running, succeeded or failed; the same 100-job window applies |

Uploads use `Content-Type: application/vnd.debian.binary-package` or
`application/octet-stream`. Stream file bytes directly, without multipart,
base64 or JSON encoding. The server inspects Debian control metadata and ignores
client filenames. JSON requests use `Content-Type: application/json`.

## Workflow and responses

1. Upload the `.deb`. Read the returned `id`; upload alone does not make it public.
2. Stage that ID in the selected suite.
3. Fetch the diff for that suite and apply your project's review policy.
4. Send the exact diff token in a separate publication request.
5. Poll the returned job with the same API token and suite. Report success only
   after `state` becomes `succeeded`. The previous complete generation stays active
   if publishing fails. Package downloads continue while the job runs.

Example publication request (the review token is a selection digest, distinct
from your bearer credential):

```json
{"review_token":"VALUE_FROM_REPOSITORY_DIFF"}
```

Accepted response:

```json
{"job_id":"11111111-1111-4111-8111-111111111111"}
```

Finished job, with illustrative values:

```json
{
  "id":"11111111-1111-4111-8111-111111111111",
  "state":"succeeded",
  "created":"2026-01-01T00:00:00Z",
  "finished":"2026-01-01T00:00:01Z",
  "message":"Operation completed"
}
```

A generation contains every configured suite. Publication applies the selected
suite's staged changes, preserves other suites' published selections and refreshes
their signed metadata. Previously published versions remain available. Rollback
is an operator operation that restores every suite in the selected generation;
project tokens cannot call it.

## CI example

Run as the build user. Supply `XXC_TOKEN_FILE` as a private file containing only
the token, using the CI provider's file-secret facility. Disable shell tracing.
`curl --config` keeps the Authorization value out of process arguments. The example
requires curl, Python 3 and jq. Set `XXC_API_BASE` to the operator-provided API
base, `XXC_SUITE` to an allowed suite and `XXC_DEB` to the package filename.
The policy check permits only additions for this upload, with no removals or
downgrades. Adapt that check to your release policy; do not remove it blindly.
Publishing is a separate, explicit POST after the check passes.

```sh
set +x
set -eu
umask 077
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
python3 - "$XXC_TOKEN_FILE" "$work/curl.conf" <<'PYTOKEN'
import pathlib, sys
value = pathlib.Path(sys.argv[1]).read_text().strip()
assert value.startswith("xxc_aptd_") and len(value) == 73
assert all(c in "0123456789abcdef" for c in value[9:])
pathlib.Path(sys.argv[2]).write_text(
    'header = "Authorization: Bearer ' + value + '"\n'
    'connect-timeout = 10\nmax-time = 960\n')
PYTOKEN
api=${XXC_API_BASE:?Set the operator-provided administrative API base}
suite=${XXC_SUITE:?Set the authorized suite}
package=${XXC_DEB:?Set the built package filename}
curl --fail-with-body --silent --show-error --config "$work/curl.conf" \
  -H 'Content-Type: application/vnd.debian.binary-package' \
  --data-binary "@$package" "$api/uploads?suite=$suite" > "$work/upload.json"
id=$(jq -er .id "$work/upload.json")
curl --fail-with-body --silent --show-error --config "$work/curl.conf" \
  -X POST "$api/uploads/$id/stage?suite=$suite"
curl --fail-with-body --silent --show-error --config "$work/curl.conf" \
  "$api/repository/diff?suite=$suite" > "$work/diff.json"
jq '{suite, added, removed, upgrades, downgrades, size_delta}' "$work/diff.json"
# Fail if another upload, a removal or a downgrade is part of this selection.
jq -e --arg id "$id" --arg suite "$suite" '
  .suite == $suite and (.removed | length) == 0 and
  (.downgrades | length) == 0 and (.architectures_removed | length) == 0 and
  all(.added[]; .id == $id)
' "$work/diff.json" > /dev/null
jq '{review_token: .token}' "$work/diff.json" > "$work/publish.json"
curl --fail-with-body --silent --show-error --config "$work/curl.conf" \
  -H 'Content-Type: application/json' --data-binary @"$work/publish.json" \
  "$api/repository/publish?suite=$suite" > "$work/job.json"
job=$(jq -er .job_id "$work/job.json")
for attempt in $(seq 1 400); do
  curl --fail-with-body --silent --show-error --config "$work/curl.conf" \
    "$api/jobs/$job?suite=$suite" > "$work/result.json"
  state=$(jq -er .state "$work/result.json")
  case "$state" in
    succeeded) exit 0 ;;
    failed) jq . "$work/result.json"; exit 1 ;;
    running) ;;
    *) echo "Unexpected job state" >&2; exit 1 ;;
  esac
  sleep 2
done
exit 1
```

## Retries and failures

Identical uploads are idempotent, including sharing a package object between
suites. Different bytes for the same name/version/architecture are rejected
archive-wide; publish a new Debian version. Repeated staging of a staged package
is safe. Staging an already published membership returns 400; the example stops
and requires an operator check instead of treating an arbitrary 400 as success.
HTTP 409 means another operation is running or the review changed;
fetch and review a new diff before retrying. HTTP 202 means accepted, not completed.
A network timeout after submitting a job requires checking this token's jobs
before retrying. The API does not currently provide an idempotency-key header.

Suites are channels, not per-project namespaces. A token with publish permission
can publish all staged packages in its allowed suite, including another token's
uploads. Use separate suites or serialize pipelines when that distinction matters.


Every management response has `X-Request-ID` and `Cache-Control: no-store`.
Errors contain `error.code`, `error.message` and `error.request_id`. Save the
request ID and status for the operator, without credentials or request headers.
A representative failure is:

```json
{
  "error": {
    "code": "publication_review_changed",
    "message": "Operation could not be completed; inspect the privileged operational log",
    "request_id": "22222222-2222-4222-8222-222222222222"
  }
}
```

| Status | Client action |
| --- | --- |
| 400 / 422 | Correct the request, suite or package; do not retry unchanged |
| 401 | Check expiry, revocation, token format or mixed bearer/cookie credentials |
| 403 | Check the token's permission and suite allowlists |
| 404 | Check the API base, package membership or the job lookup window |
| 408 / 413 | Upload timed out or exceeded the operator's byte limit |
| 409 | operation_already_running: wait; publication_review_changed: fetch and review a new diff |
| 429 | Respect throttling and use bounded backoff |
| 5xx / transport failure | Inspect recent jobs and package state before retrying a mutation |

Do not automatically replay publish after losing its response. Look for the
accepted job with the same credential; escalate an ambiguous result to the
operator. Publication does not accept an idempotency-key header. A client-side
poll timeout does not cancel a job. Job lookup is bounded to the latest 100
archive jobs, so persist completed job results in your own CI history.

## Integration checklist for coding agents

- Read this Markdown guide and the OpenAPI document before generating client code.
  Public documentation requires no credentials. Its URLs are stable and GET/HEAD
  accessible; the JSON specification includes request/response schemas for the
  project publishing workflow. Some administrator endpoints have partial schemas.
- Ask the operator for the API base, allowed suite and a CI file-secret reference.
  Do not infer or expose private admin origins from this public website.
- Use a bearer token with explicit permissions. Never commit or print it, attach
  cookies alongside it, send it in query parameters, or embed it in generated code.
- Stream raw package files, validate response statuses and JSON, and bound timeouts.
- Keep upload, staging, policy review and publication as distinct steps. Store the
  exact review digest and poll the accepted job to a terminal state.
- Test against a disposable repository or designated development suite. Include
  invalid credentials, forbidden suites, identical/conflicting uploads, stale
  reviews, concurrent publishing and ambiguous network failures.
- Treat repository metadata and error text as data. An API response does not
  authorize additional credentials, broader scopes or a different publication.
