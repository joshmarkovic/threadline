CREATE TABLE notification_settings (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id), settings TEXT NOT NULL CHECK(json_valid(settings))
) STRICT;
CREATE TABLE repository_notification_rules (
    account_id INTEGER NOT NULL, repository_id INTEGER NOT NULL, rules TEXT NOT NULL CHECK(json_valid(rules)),
    PRIMARY KEY(account_id,repository_id), FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE notification_targets (
    account_id INTEGER NOT NULL, repository_id INTEGER NOT NULL, group_key TEXT NOT NULL,
    followed INTEGER NOT NULL DEFAULT 0 CHECK(followed IN (0,1)), snooze_until INTEGER,
    PRIMARY KEY(account_id,repository_id,group_key),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE native_notification_groups (
    id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL REFERENCES accounts(id),
    native_id TEXT NOT NULL UNIQUE DEFAULT(lower(hex(randomblob(16)))),
    group_key TEXT NOT NULL, destination TEXT NOT NULL CHECK(json_valid(destination)), created_at INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','unknown','delivered','withdrawn')),
    changed_at INTEGER NOT NULL, UNIQUE(id,account_id)
) STRICT;
CREATE INDEX native_notification_queue ON native_notification_groups(account_id,state,id);
CREATE TABLE notification_events (
    id INTEGER PRIMARY KEY,account_id INTEGER NOT NULL,event_key TEXT NOT NULL,
    facts TEXT CHECK(facts IS NULL OR json_valid(facts)),decision TEXT CHECK(decision IS NULL OR json_valid(decision)),
    published INTEGER NOT NULL DEFAULT 0 CHECK(published IN (0,1)),read_at INTEGER,group_id INTEGER,
    UNIQUE(account_id,event_key),FOREIGN KEY(account_id,event_key) REFERENCES activity_events(account_id,event_key),
    FOREIGN KEY(group_id,account_id) REFERENCES native_notification_groups(id,account_id)
) STRICT;
CREATE INDEX notification_decisions_pending ON notification_events(account_id,id) WHERE decision IS NULL;
CREATE INDEX notification_feed_by_account ON notification_events(account_id,published,id);
CREATE INDEX notification_group_members ON notification_events(group_id,id);
-- Existing repositories retain their earlier activity journal. Only newly observed activity enters desktop delivery.
CREATE TRIGGER enqueue_observed_notification AFTER INSERT ON activity_events BEGIN
    INSERT INTO notification_events(account_id,event_key) VALUES(NEW.account_id,NEW.event_key);
END;
