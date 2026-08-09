-- Migration 0011: Subscription traffic records
--
-- Adds the subscription_traffic table (M6 Slice 5): append-only traffic
-- observations per subscription, aggregated at read time for quota enforcement
-- (OUT-010/OUT-011) and the `subscription-userinfo` response header.
--
-- source_kind discriminator (single char, matched in domain TrafficSourceKind):
--   A = AirportHeader    (parsed from upstream subscription-userinfo)
--   M = ManualCorrection (admin input)
--   P = Probe            (reserved for M7, not populated in M6)
--
-- M6 does not infer real proxy traffic from download counts (terminology
-- §116-121). Only explicit records feed quota enforcement. See
-- docs/plan/milestones/M6-subscription-distribution.md §"Traffic and expiry
-- policy framework".

CREATE TABLE subscription_traffic (
    id              TEXT    PRIMARY KEY,
    subscription_id TEXT    NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    source_kind     TEXT    NOT NULL CHECK (source_kind IN ('A', 'M', 'P')),
    upload          INTEGER NOT NULL DEFAULT 0,
    download        INTEGER NOT NULL DEFAULT 0,
    recorded_at     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    source_ref      TEXT    NOT NULL DEFAULT ''
);

CREATE INDEX idx_subscription_traffic_subscription ON subscription_traffic(subscription_id);
