# OpenPGP key rotation

Never remove old client trust before the replacement key has been distributed.
The current signer exports one selected key per publication; overlapping public
keyring distribution is an operator-managed step until managed key rotation is
implemented. Do not treat changing aptd.conf alone as a safe rotation workflow.

1. Back up configuration, private keys and the complete archive offline.
2. Generate/import the next key locally or create it in XXC Trust, retaining the
   old key and fingerprint. Use the CA operator workflow for private-key imports;
   XXC-APTD has no private import/export route.
3. Build a public keyring containing both keys using GPG public export only.
4. Distribute that keyring to existing clients through a separately trusted
   configuration-management channel or an archive-keyring package signed by the
   old key. Verify adoption before changing the signing fingerprint.
5. Change the backend, remote key ID if applicable, and fingerprint in aptd.conf;
   validate and restart, then publish.
6. Check both old-trust-plus-new-trust and newly configured clients with real APT.
7. Retain generation public-key snapshots as long as those generations can be
   rolled back. Verification is offline and independent of the active backend.
8. Remove obsolete client trust only after the migration window and rollback
   policy permit it. Revoke compromised keys through a separate incident plan.

Never expose private exports via web/API. XXC Trust X.509 certificate rotation is unrelated
to the APT repository signing trust chain.

Offline generation verification uses the public key snapshot saved at
publication. It does not fetch later CA revocations. For compromise response,
distribute updated trust/revocations to clients and explicitly restrict rollback
to acceptable generations. Exchange listing is optional and independent of
signing-key activation; removing a listing does not recall downloaded copies.
