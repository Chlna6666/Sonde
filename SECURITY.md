# Security Policy

## Supported versions

Sonde is still in active development. Security fixes are made on `main` and on the latest published release when one exists. Older development snapshots are not maintained as separate security branches.

## Reporting a vulnerability

Please do not publish exploit details, credentials, tokens, database URLs, backup archives, or other sensitive material in a public issue.

Prefer GitHub's private vulnerability reporting / repository Security Advisory flow when it is available for this repository. If that private channel is unavailable, open a minimal public issue asking for a private contact channel without including reproduction details that would enable exploitation.

A useful report includes:

- the affected Sonde commit or release;
- deployment topology (direct bind, reverse proxy, Docker, database backend);
- the security boundary that is crossed;
- minimal reproduction steps or a proof of concept;
- impact and any known preconditions;
- whether the issue is already public or has a disclosure deadline.

Do not include real production secrets. Replace them with synthetic values.

## Scope

Security-sensitive areas include authentication and 2FA, session/CSRF handling, RBAC and application isolation, ingest authentication/signatures/replay protection, backup and restore, D1 import parsing, outbound webhook/notification SSRF controls, proxy trust, filesystem confinement, embedded web assets, container/runtime hardening, and dependency/supply-chain issues.

The CI dependency audits are blocking checks. A known advisory must be fixed or have an explicit, documented compensating-control decision; silent advisory ignores are not accepted.
