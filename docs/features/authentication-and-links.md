# Authentication and subscription links

The login page accepts a username/password and, when enabled, a TOTP or
recovery code. Pending requests cannot be submitted again. Passwords use
Argon2id with individual salts; login failures use a generic response for an
unknown username, a wrong password or an inactive account. Failure counters
cover both username and canonical client IP. Excess password verification
work returns 429 instead of growing an unbounded Argon2 workload.

Session cookies are HttpOnly and SameSite=Lax. Deploy remote access behind
HTTPS and set `security.cookie_secure = true`; plain local HTTP remains
supported. Leave `security.trust_proxy_headers = false` for direct access.
Enable it only behind a proxy that controls the forwarded headers and restrict
backend access to that proxy. The transport peer remains the fallback for
missing or invalid proxy addresses. Auth/admin API responses are no-store;
HTML cannot be framed and does not forward referrer URLs.

Subscription links are bearer credentials: anyone holding a working link can
read that subscription. The normal and temporary tokens contain 32 random
bytes from the OS CSPRNG (256 bits, 43 Base64URL characters). Only a
purpose-separated HMAC-SHA256 digest is stored, using the persisted master
key. They do not derive from usernames, passwords, slugs or ULIDs. This
construction does not need a JWT replacement or a new encryption scheme.
See the [OWASP random session credential guidance](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#session-id-entropy)
for the CSPRNG and entropy baseline.

New short codes use 22 unbiased base62 characters (about 131 bits). They also
grant access and must be kept private. They remain stored in plaintext so an
authenticated administrator can retrieve and copy the existing short link;
therefore protect database backups too. Failed short-code lookups have their
own bounded IP limiter and cannot exhaust the administrator login counters.
Old short codes remain valid; regenerate them explicitly to obtain the new
length. Do not publish full links in logs, screenshots or support requests.

Concurrent short-code regenerations replace the current credential in commit
order. Only the last committed code remains valid; an overlapping request does
not exhaust random-code retries or leave the previous credential active. A
failed replacement leaves the previous code usable.

The subscription list's Copy action fetches the existing short code and
copies `/s/{code}/{profile}`. If no short code exists, generate one first.
Creation, token rotation and temporary-link dialogs display an importable
`/sub/{token}/{profile}` URL once. The slug is only a name, never a credential.

Web token rotation asks for confirmation and immediately invalidates the old
token URL. Update clients to use the new URL. The API still supports an
explicit grace period, including permanent grace with `null`/`-1`; the CLI's
existing default is also permanent grace. For incident recovery specify zero
grace. Short codes and temporary links are independent credentials: regenerate
or revoke those separately if they leaked. Rotating the master key is not a
routine way to revoke one subscription; it affects other persisted secrets.

The owning semantics are in the [M2 blueprint](../plan/milestones/M2-auth-and-users.md),
[M6 blueprint](../plan/milestones/M6-subscription-distribution.md) and
[HTTP boundary contract](../contracts/module-boundaries.md). Verification maps
to AUTH-004/009, SEC-007/009/010 and OUT-013. These changes require a containing
build; the published `v0.1.0` image does not include this hardening.
