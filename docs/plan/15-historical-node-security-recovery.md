# Historical node security recovery (0030 / NODE-010 / GEN-015)

Only an explicit operator-reviewed mapping may identify affected old NodeIds.
No name, host, array position or current reimport can infer that association.
Migration 0030 creates an empty recovery-state table; it does not disable all
historical nodes. A populated historical pool requires a reviewed mapping,
including an explicitly empty result when no affected node was confirmed.

The mapping binds the existing master-key fingerprint, pool revision and each
old NodeId, row revision and identity fingerprint. Its preview names every
affected ID and action before staging changes. Quarantine preserves the node
row, credentials, tags, override/disabled settings, source bindings and every
chain, historical template and subscription reference. It affects distribution
eligibility independently of the user's enabled state; dependent chains are
also ineligible. Fixed required references fail clearly, never retarget.
Unrelated nodes remain eligible and can generate new output.

Repair requires that exact old ID to already be quarantined.
Recovery consumes a hash-verified original artifact and explicit entry mapping.
It parses through the current backend and changes security projection in place
on the same old ID under compare-and-swap checks. It never calls ordinary
import/reconcile to create a replacement ID. Only unambiguous, supported
security-only differences with preserved endpoint/authentication and non-security
identity may be applied automatically; other differences remain quarantined for
separate review. TLS policy types are not interchangeable. Identity collisions,
stale revisions/fingerprints, wrong keys, absent inputs or contradictory
provenance reject the entire staging action without releasing quarantine.

Quarantine/repair changes pool revision and the withdrawal floor atomically.
The floor is global: all older generated caches must rebuild because the old
schema records no reliable output-to-node lineage. History and ciphertext are
retained; no old payload is relabeled safe. This can prevent an unrelated old
fallback until regeneration, and the preview must disclose that impact. Explicit
withdrawal always wins over last-good fallback.

The existing private rollback/staging workflow applies to mapping actions even
when the schema is current. Read-only preview and actual application use the
same prepared action set. Reapplication is idempotent only when the recorded
action, evidence and resulting state agree; it cannot silently accept another
operation's state. Recovery settings/references must compare equal before and
after the synthetic tests. Production operations and publication require their
separate authorization.

Scope and authority: this chapter owns the migration policy for explicitly
confirmed historical IDs; domain types and the module-boundary contract define
the operation, SQLite owns the atomic state transition, CLI owns private staging
and artifact preparation. It does not authorize production migration, parser
inference, protocol replacement, deployment or publication.

Verification entrypoints: storage `node_security_recovery`, CLI
`node_security_recovery` and application `generation_pipeline_regression`
security_recovery cases bind NODE-010/GEN-015. They cover reference/ciphertext
preservation, transitive chains, original-ID TLS repair, CAS/hash/key failures,
collision, ignored updates, replay, empty review, readonly preview and selector
scope. A passing synthetic suite does not prove inspection of any production
pool or recover unavailable historical final-source bytes.
