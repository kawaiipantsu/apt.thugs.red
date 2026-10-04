# Public interface

See the [screenshot gallery](SCREENSHOTS.md) for public, administrative and mobile
pages captured from an isolated fixture repository.

The original project favicon and locally embedded CSS/JavaScript use a near-black
surface, thin gray borders, bright red accents and a system monospace stack.
No CDN or mandatory Node runtime is used. CSS variables in
`web/static/css/site.css` define accent/background/foreground/muted/border/panel
and success/warning/error colors. Navigation, forms, files and package links work
without JavaScript. Public JavaScript enhances command copying; admin JavaScript
polls background job status.

Routes: `/`, `/packages`, `/packages/{name}`, `/search`, `/releases`,
`/releases/{suite}`, `/about`, `/help`, `/repo/`. Directory pages offer names,
types, sizes and parent navigation. Raw files always retain their original bytes.
Search matches public package names/descriptions with FTS5 and pages of 50.
Package details show metadata, dependencies, checksums and direct downloads.

Visible keyboard focus, a skip link, semantic labels and table headers support
keyboard/assistive navigation. Mobile layouts collapse cards and metadata rows;
wide directory tables scroll. Motion is suppressed under prefers-reduced-motion.

Complete directory sorting/filtering and metadata previews, automatic
Debian version selection, full retained-version views and package sitemap shards
remain roadmap work. The current sitemap covers primary static routes only.

## Administrative interface

The separate admin listener uses the same local design tokens and server-rendered
forms. Login, dashboard, packages, uploads, staging, publication review, retained
generations, jobs, signing fingerprint, users, audit, effective configuration and
basic operational health are available. Configuration is read-only. User forms
and mutation controls depend on role; enforcement remains on the server.

Publication shows additions, version comparisons, architecture changes and size
delta before an explicit submit. A stale review cannot publish. Job results have
a manual refresh link and optional JavaScript polling. The upload form streams
one .deb, with CSRF validation before package data. Browser tests exercise the
workflow without JavaScript and check mobile layout, keyboard focus and axe
accessibility. See [ADMINISTRATION.md](ADMINISTRATION.md).

The dashboard does not yet show disk capacity, key expiry, uptime or all requested
operational metrics. Managed signing operations and package removal are pending.

The administrator-only XXC Trust pages present connection state, authorities,
templates and searchable paginated certificates. They share the local visual
theme, keyboard navigation and escaped server-side rendering.

Administrators can generate CA-held OpenPGP keys and download public exports on
`/admin/keys`. The form explains public identity and ambiguous timeout behavior;
activation requires an explicit server configuration change. The browser suite
exercises remote signing and key management without JavaScript.

The layout reserves scrollbar space, with an always-scroll fallback. The admin
dashboard renders responsive SVG curves, sparklines, a request mix ring and
ranked bars with keyboard inspection and equivalent daily tables. See
[ANALYTICS.md](ANALYTICS.md). Suite workflow selectors and administrator token
creation/revocation work without JavaScript. Assets and browser APIs use the
/admin prefix for a single proxy location.


## Public API guide

The `/api` link appears in the shared public navigation and footer. The guide
renders `docs/AUTOMATION.md` into HTML with a table of contents, responsive tables
and copyable examples. Markdown and OpenAPI links let developers and coding
agents fetch the same reference directly. Reading and anchor navigation work
without JavaScript; copy controls appear when JavaScript is available. Code and
reference tables scroll within the content column at mobile widths.
