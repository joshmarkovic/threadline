CREATE TABLE sync_settings (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id),
    schedule TEXT NOT NULL CHECK(json_valid(schedule)),
    background_paused INTEGER NOT NULL CHECK(background_paused IN (0,1)),
    notification_quiet INTEGER NOT NULL CHECK(notification_quiet IN (0,1))
) STRICT;
CREATE TABLE sync_budgets (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id),
    not_before INTEGER NOT NULL DEFAULT 0,
    next_slot INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE scheduled_reads (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    repository_id INTEGER NOT NULL,
    item_id INTEGER,
    family TEXT NOT NULL,
    revision TEXT NOT NULL,
    due_at INTEGER NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0,
    lease_until INTEGER,
    leased_generation INTEGER,
    last_started INTEGER,
    last_success INTEGER,
    failure TEXT,
    failures INTEGER NOT NULL DEFAULT 0,
    suspended INTEGER NOT NULL DEFAULT 0 CHECK(suspended IN (0,1)),
    cursor TEXT,
    watermark INTEGER,
    page_watermark INTEGER,
    walk_started INTEGER,
    etag TEXT,
    last_modified TEXT,
    validator_revision TEXT,
    validator_since INTEGER,
    walk_etag TEXT,
    walk_modified TEXT,
    walk_revision TEXT,
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id),
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id)
) STRICT;
CREATE UNIQUE INDEX scheduled_target ON scheduled_reads(account_id,repository_id,COALESCE(item_id,0),family,revision);
CREATE INDEX scheduled_due ON scheduled_reads(account_id,due_at);
CREATE TABLE sync_subscriptions (
    read_id INTEGER NOT NULL REFERENCES scheduled_reads(id) ON DELETE CASCADE,
    subscriber TEXT NOT NULL,
    priority INTEGER NOT NULL CHECK(priority BETWEEN 0 AND 4),
    PRIMARY KEY(read_id,subscriber)
) STRICT, WITHOUT ROWID;
CREATE VIEW eligible_sync_repositories AS
SELECT ra.account_id, ra.repository_id, p.credential_generation
FROM github_repository_access ra
JOIN github_access_profiles p USING(account_id,owner_scope)
JOIN active_profile active ON active.account_id = ra.account_id
JOIN dispatch_profiles dp ON dp.account_id = ra.account_id
WHERE ra.selected = 1 AND ra.access = 1 AND p.access = 1
  AND ra.verified_generation = p.credential_generation AND p.installed_generation = p.credential_generation
  AND dp.authorized = 1 AND dp.offline = 0;
CREATE TABLE feed_object_versions (
    account_id INTEGER NOT NULL,
    repository_id INTEGER NOT NULL,
    category TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    state TEXT,
    PRIMARY KEY(account_id,repository_id,category,provider_id),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;

-- A reply which crossed a sign-out, switch or selection change cannot become valid again.
CREATE TRIGGER invalidate_deselected_reads AFTER UPDATE OF selected,access,verified_generation ON github_repository_access
WHEN OLD.selected <> NEW.selected OR OLD.access <> NEW.access OR OLD.verified_generation IS NOT NEW.verified_generation
BEGIN
    UPDATE scheduled_reads SET lease_until=NULL
    WHERE account_id=OLD.account_id AND repository_id=OLD.repository_id AND lease_until IS NOT NULL;
END;
CREATE TRIGGER invalidate_profile_reads AFTER UPDATE OF credential_generation,access ON github_access_profiles
WHEN OLD.credential_generation<>NEW.credential_generation OR OLD.access<>NEW.access
BEGIN
    UPDATE scheduled_reads SET lease_until=NULL WHERE account_id=NEW.account_id AND lease_until IS NOT NULL
    AND repository_id IN (SELECT repository_id FROM github_repository_access
                          WHERE account_id=NEW.account_id AND owner_scope=NEW.owner_scope);
END;
CREATE TRIGGER invalidate_paused_reads AFTER UPDATE OF offline,authorized ON dispatch_profiles
WHEN NEW.offline=1 OR NEW.authorized=0
BEGIN
    UPDATE scheduled_reads SET lease_until=NULL WHERE account_id=NEW.account_id AND lease_until IS NOT NULL;
END;
CREATE TRIGGER invalidate_inactive_reads AFTER UPDATE OF account_id ON active_profile
WHEN OLD.account_id<>NEW.account_id
BEGIN
    UPDATE scheduled_reads SET lease_until=NULL WHERE account_id=OLD.account_id AND lease_until IS NOT NULL;
END;
