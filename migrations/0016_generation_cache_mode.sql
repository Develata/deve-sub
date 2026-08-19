-- Add `mode` column to generation_cache (B-10).
-- WHY: the cache key now includes GenerationMode (strict vs lenient) so that
-- a lenient generation cannot be served to a strict request (which would
-- bypass the strict exclusion check). Existing rows are backfilled to
-- 'lenient' (the GenerationMode default), which is safe because all
-- pre-migration cached entries were generated without mode in the key and
-- treating them as lenient is the conservative choice.
ALTER TABLE generation_cache ADD COLUMN mode TEXT NOT NULL DEFAULT 'lenient';
