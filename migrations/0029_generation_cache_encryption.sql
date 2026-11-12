-- Preserve last-good identities and selectors while an explicit keyed migration
-- seals legacy payloads. Readers must never fall back to content_legacy.
ALTER TABLE generation_cache RENAME COLUMN content TO content_legacy;
ALTER TABLE generation_cache ADD COLUMN content_encrypted TEXT;

-- An old writer cannot reintroduce plaintext after the schema transition.
-- Existing rows remain recoverable until keyed backfill commits atomically.
CREATE TRIGGER generation_cache_encrypted_insert
BEFORE INSERT ON generation_cache
WHEN NEW.content_legacy <> '' OR NEW.content_encrypted IS NULL
     OR substr(NEW.content_encrypted, 1, 3) <> 'v2:'
BEGIN
    SELECT RAISE(ABORT, 'generation cache requires encrypted content');
END;

CREATE TRIGGER generation_cache_encrypted_update
BEFORE UPDATE ON generation_cache
WHEN NEW.content_legacy <> '' OR NEW.content_encrypted IS NULL
     OR substr(NEW.content_encrypted, 1, 3) <> 'v2:'
BEGIN
    SELECT RAISE(ABORT, 'generation cache requires encrypted content');
END;
