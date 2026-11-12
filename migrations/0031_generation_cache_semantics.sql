-- Encryption authenticates bytes, not the safety of the historical producer.
-- ADD with DEFAULT does not fire 0029's update trigger on pending plaintext.
ALTER TABLE generation_cache ADD COLUMN semantics_version INTEGER NOT NULL
    DEFAULT 0 CHECK (semantics_version IN (0, 7));
