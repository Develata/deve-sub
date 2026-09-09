-- Lifetime totals are projections of accepted non-negative traffic deltas.
-- SQLx applies this forward migration transactionally; an invalid historical
-- integer or overflow aborts the upgrade rather than silently changing quotas.
CREATE TABLE traffic_totals (
    subscription_id TEXT NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('A', 'M', 'P')),
    upload INTEGER NOT NULL CHECK (typeof(upload) = 'integer' AND upload >= 0),
    download INTEGER NOT NULL CHECK (typeof(download) = 'integer' AND download >= 0),
    PRIMARY KEY (subscription_id, source_kind)
);
CREATE TABLE probe_traffic_totals (
    subscription_id TEXT NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    prefix TEXT NOT NULL,
    upload INTEGER NOT NULL CHECK (typeof(upload) = 'integer' AND upload >= 0),
    download INTEGER NOT NULL CHECK (typeof(download) = 'integer' AND download >= 0),
    PRIMARY KEY (subscription_id, prefix)
);

-- Validate individual old samples before aggregation; CHECK also detects SUM
-- overflow. Temporary guard has no persistent schema role.
CREATE TEMP TABLE traffic_migration_guard (
    valid INTEGER NOT NULL CHECK (valid = 1)
);
INSERT INTO traffic_migration_guard
SELECT CASE WHEN EXISTS (SELECT 1 FROM subscription_traffic
    WHERE upload < 0 OR download < 0 OR typeof(upload) != 'integer'
        OR typeof(download) != 'integer') THEN 0 ELSE 1 END;
DROP TABLE traffic_migration_guard;

INSERT INTO traffic_totals
SELECT subscription_id, source_kind, SUM(upload), SUM(download)
FROM subscription_traffic GROUP BY subscription_id, source_kind;
INSERT INTO probe_traffic_totals
SELECT subscription_id,
    CASE WHEN instr(source_ref, ':') > 0
         THEN substr(source_ref, 1, instr(source_ref, ':') - 1) ELSE source_ref END AS prefix,
    SUM(upload), SUM(download)
FROM subscription_traffic WHERE source_kind = 'P' GROUP BY subscription_id, prefix;

-- Reconstruct days with raw evidence; preserve any older snapshot-only days.
INSERT INTO traffic_daily_snapshots
    (subscription_id, date, total_upload, total_download, source_breakdown_json, computed_at)
SELECT subscription_id, date, SUM(upload), SUM(download),
    json_group_object(source_kind, json_array(upload, download)),
    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM (
    SELECT subscription_id, substr(recorded_at, 1, 10) AS date, source_kind,
        SUM(upload) AS upload, SUM(download) AS download
    FROM subscription_traffic GROUP BY subscription_id, date, source_kind
) WHERE true GROUP BY subscription_id, date
ON CONFLICT(subscription_id, date) DO UPDATE SET
    total_upload = excluded.total_upload, total_download = excluded.total_download,
    source_breakdown_json = excluded.source_breakdown_json, computed_at = excluded.computed_at;

CREATE TRIGGER traffic_validate_delta BEFORE INSERT ON subscription_traffic BEGIN
    SELECT CASE WHEN NEW.upload < 0 OR NEW.download < 0
        OR typeof(NEW.upload) != 'integer' OR typeof(NEW.download) != 'integer'
        THEN RAISE(ABORT, 'traffic delta must be a non-negative integer') END;
END;
CREATE TRIGGER traffic_daily_validate_insert BEFORE INSERT ON traffic_daily_snapshots BEGIN
    SELECT CASE WHEN NEW.total_upload < 0 OR NEW.total_download < 0
        OR typeof(NEW.total_upload) != 'integer' OR typeof(NEW.total_download) != 'integer'
        THEN RAISE(ABORT, 'daily traffic integer overflow') END;
END;
CREATE TRIGGER traffic_daily_validate_update BEFORE UPDATE ON traffic_daily_snapshots BEGIN
    SELECT CASE WHEN NEW.total_upload < 0 OR NEW.total_download < 0
        OR typeof(NEW.total_upload) != 'integer' OR typeof(NEW.total_download) != 'integer'
        THEN RAISE(ABORT, 'daily traffic integer overflow') END;
END;
CREATE TRIGGER traffic_project_delta AFTER INSERT ON subscription_traffic BEGIN
    INSERT INTO traffic_totals VALUES
        (NEW.subscription_id, NEW.source_kind, NEW.upload, NEW.download)
    ON CONFLICT(subscription_id, source_kind) DO UPDATE SET
        upload = upload + excluded.upload, download = download + excluded.download;
    INSERT INTO probe_traffic_totals
        SELECT NEW.subscription_id,
            CASE WHEN instr(NEW.source_ref, ':') > 0
                 THEN substr(NEW.source_ref, 1, instr(NEW.source_ref, ':') - 1)
                 ELSE NEW.source_ref END, NEW.upload, NEW.download
        WHERE NEW.source_kind = 'P'
    ON CONFLICT(subscription_id, prefix) DO UPDATE SET
        upload = upload + excluded.upload, download = download + excluded.download;
    INSERT INTO traffic_daily_snapshots
        (subscription_id, date, total_upload, total_download, source_breakdown_json)
        VALUES (NEW.subscription_id, substr(NEW.recorded_at, 1, 10), NEW.upload, NEW.download,
            json_object(NEW.source_kind, json_array(NEW.upload, NEW.download)))
    ON CONFLICT(subscription_id, date) DO UPDATE SET
        total_upload = total_upload + NEW.upload,
        total_download = total_download + NEW.download,
        source_breakdown_json = json_set(source_breakdown_json,
            '$.' || NEW.source_kind,
            json_array(
                COALESCE(json_extract(source_breakdown_json, '$.' || NEW.source_kind || '[0]'), 0) + NEW.upload,
                COALESCE(json_extract(source_breakdown_json, '$.' || NEW.source_kind || '[1]'), 0) + NEW.download)),
        computed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now');
END;

-- Historical crash recovery left terminal runs without completion times.
-- Start their diagnostic retention window at upgrade rather than deleting
-- potentially valuable failure evidence immediately based on creation time.
UPDATE probe_runs SET completed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
WHERE status IN ('C', 'X', 'F') AND completed_at IS NULL;

CREATE INDEX idx_traffic_retention ON subscription_traffic(recorded_at);
CREATE INDEX idx_probe_runs_retention ON probe_runs(status, completed_at) WHERE status IN ('C', 'X', 'F');
CREATE INDEX idx_refresh_jobs_retention ON source_refresh_jobs(status, finished_at) WHERE status IN ('C', 'F', 'X');
CREATE INDEX idx_sessions_retention ON sessions(expires_at);
CREATE INDEX idx_temp_links_retention ON subscription_temp_links(expires_at);
CREATE INDEX idx_outbox_retention ON outbox_event(processed_at) WHERE processed_at IS NOT NULL;
