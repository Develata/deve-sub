# Security Policy

## Status

Deve Sub is a self-hosted proxy subscription infrastructure manager under
active development. It is not yet a tagged release. Do not expose development
builds to untrusted networks.

## Sensitive data rules

Never commit:

- subscription source URLs, cookies, custom request headers, or any upstream
  credentials;
- subscription tokens, session secrets, recovery codes, or API keys;
- master encryption keys, `.env` files, or private keys;
- real proxy node credentials in fixtures or tests (use the reserved test
  addresses documented in `docs/plan/05-protocol-engine.md`);
- real user traffic data, audit logs, or probe snapshots.

Fixtures must use reserved test identifiers (for example the documentation
UUID `00000000-0000-4000-8000-000000000001` and the documentation IPv6 prefix
`2001:db8::/32`).

## Reporting

Report security issues privately to the repository maintainer. Update this file
with a public contact and disclosure policy when the repository moves to public
visibility or a tagged release.
