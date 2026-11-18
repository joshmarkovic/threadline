-- Durable intents refer to the existing immutable Outbox snapshot. Legacy entries remain inspectable.
CREATE TABLE write_intents (
    account_id INTEGER NOT NULL,
    operation_id TEXT NOT NULL,
    issue_api_id TEXT NOT NULL,
    submitted_at INTEGER NOT NULL,
    PRIMARY KEY(account_id, operation_id),
    FOREIGN KEY(account_id, operation_id) REFERENCES operations(account_id, id)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER write_immutable_payload BEFORE UPDATE OF snapshot,projection,payload_version ON operations
WHEN EXISTS(SELECT 1 FROM write_intents w WHERE w.account_id=OLD.account_id AND w.operation_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'submitted write intent is immutable'); END;

CREATE TABLE write_evidence (
    account_id INTEGER NOT NULL,
    operation_id TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    credential_generation INTEGER NOT NULL,
    observed_at INTEGER NOT NULL,
    current_fields TEXT NOT NULL CHECK(json_valid(current_fields)),
    route TEXT NOT NULL CHECK(json_valid(route)),
    result TEXT NOT NULL,
    PRIMARY KEY(account_id,operation_id,attempt),
    FOREIGN KEY(account_id,operation_id,attempt) REFERENCES operation_attempts(account_id,operation_id,attempt)
) STRICT, WITHOUT ROWID;

CREATE TABLE write_timing (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id),
    not_before INTEGER NOT NULL,
    retry_epoch INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE write_accepted_objects (
    account_id INTEGER NOT NULL,
    operation_id TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    kind TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    PRIMARY KEY(account_id,operation_id,attempt),
    FOREIGN KEY(account_id,operation_id,attempt) REFERENCES operation_attempts(account_id,operation_id,attempt)
) STRICT, WITHOUT ROWID;

CREATE TABLE own_write_actions (
    account_id INTEGER NOT NULL,
    operation_id TEXT NOT NULL,
    repository_id INTEGER NOT NULL,
    item_id INTEGER NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('issue_edited','comment_created','comment_edited')),
    remote_id TEXT NOT NULL,
    remote_revision TEXT NOT NULL,
    confirmed_at INTEGER NOT NULL,
    PRIMARY KEY(account_id,operation_id),
    FOREIGN KEY(account_id,operation_id) REFERENCES operations(account_id,id),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id),
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX own_write_identity ON own_write_actions(account_id,repository_id,action,remote_id,remote_revision);
