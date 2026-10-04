# Screenshot gallery

XXC-APTD 0.1.0, captured from the running application using disposable packages,
accounts, synthetic traffic and a local signing fixture. These images show the implemented interface;
remaining features are listed in the [roadmap](ROADMAP.md).

Desktop captures use a 1440-pixel viewport; mobile captures use 390 pixels.
Signing fingerprints are masked. Client setup commands use a reserved example
domain. No production account, credential, private key or internal address appears.
Run `make screenshots` to regenerate the captures; see [Development](DEVELOPMENT.md).

## Public repository

### Home

Repository status, search, recent packages and client setup.

![Public repository home](screenshots/public-home.png)

### Package index and detail

| Searchable index | Package metadata and download |
| --- | --- |
| ![Package index](screenshots/public-packages.png) | ![Package details](screenshots/public-package.png) |

### Directory browser

Signed metadata remains available at its normal APT URL.

![Repository directory containing InRelease and Release metadata](screenshots/public-repository.png)

### Client setup

![Repository key and Deb822 setup instructions](screenshots/public-setup.png)

### Developer API guide

Public integration instructions, copyable examples and Markdown/OpenAPI downloads.
The first viewport is shown; the full page includes endpoint, retry and coding-agent guidance.

![Public API developer guide](screenshots/public-api.png)

## Administration

### Login and dashboard

| Local account login | Repository control |
| --- | --- |
| ![Empty admin login form](screenshots/admin-login.png) | ![Authenticated repository dashboard](screenshots/admin-dashboard.png) |

### Project API tokens

Administrators create expiring credentials with explicit permissions and suites.
No token value is included in these captures.

![Project API token management](screenshots/admin-tokens.png)

### Upload and review

| Inspected upload | Reviewed version upgrade |
| --- | --- |
| ![Package upload and inspected fixture](screenshots/admin-uploads.png) | ![Publication diff before explicit approval](screenshots/admin-publish.png) |

### Completed publication

![Successful background publication job](screenshots/admin-job.png)

### Remote key management

Generation and public export through XXC Trust. Activation remains a separate,
explicit server configuration change.

![Remote OpenPGP key generation form and public inventory](screenshots/admin-keys.png)

## Mobile

| Public home | Publication review |
| --- | --- |
| ![Public home at mobile width](screenshots/public-mobile.png) | ![Admin publication review at mobile width](screenshots/admin-mobile.png) |

### Traffic on mobile

Charts and rankings below use synthetic history in a disposable fixture database.
The installed service records actual traffic only.

![Responsive traffic dashboard](screenshots/admin-statistics-mobile.png)
