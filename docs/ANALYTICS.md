# Traffic statistics

The authenticated dashboard at `/admin/` shows public origin traffic: downloads,
estimated distinct clients, bytes, distinct assets, daily smooth curves, request
mix, popular packages and popular assets. Choose 7/30/90 days and a measure.
Keyboard arrows, Home and End inspect chart points. Expand daily values for a
plain table; charts and navigation render without JavaScript.

A download is a GET for a published `.deb` whose HTTP 200 body was fully handed
to the HTTP transport. This is not proof of installation or receipt by the client.
Range responses (206), conditional responses (304), errors and interrupted streams
are counted separately. HEAD, administrative traffic and health probes are excluded.
Bytes count body data handed to HTTP, excluding headers and TLS overhead.
Upstream cache hits never reach the daemon and are absent from these figures.

Distinct clients are estimates based on HMAC-SHA256 of canonical IP addresses,
using a private random key in SQLite. No analytics cookies are set. Raw addresses,
user agents, query strings, session values and authorization headers are not stored.
Shared NAT combines clients; bots count too. The same hash deduplicates across
days in the selected retention window. Treat hashes and backups as private data.
Asset paths are recorded only for successfully served static or repository files.

Configure `[server].trusted_proxies` for the immediate reverse proxy peers. The
rightmost untrusted address in a valid forwarding chain identifies the client.
Untrusted peers cannot supply a different client identity. This setting never
authenticates an administrator or changes login throttling.

Collection uses a bounded queue and batched SQLite writes outside request handling.
It does not buffer package bodies or block downloads on database writes. Overload
or database failures may drop observations; the dashboard reports dropped events
since startup. Graceful shutdown flushes the queue; a process crash may lose its
unflushed second of observations. Daily retention removes aggregates and hashes.

The API is `GET /api/v1/analytics?days=30` on the private listener, also available
under `/admin/api/v1/analytics` for path proxies. Viewer sessions and the local
socket may read it; project bearer tokens cannot. There is no public analytics API.
