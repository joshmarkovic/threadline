CREATE TABLE merge_policies (
    revision_id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL, item_id INTEGER NOT NULL,
    observed_at INTEGER NOT NULL, policy TEXT NOT NULL CHECK(json_valid(policy)),
    FOREIGN KEY(revision_id,account_id,item_id) REFERENCES pr_revisions(id,account_id,item_id)
) STRICT;

CREATE TABLE merge_intents (
    account_id INTEGER NOT NULL, operation_id TEXT NOT NULL, item_id INTEGER NOT NULL,
    revision_id INTEGER NOT NULL, pr_api_id TEXT NOT NULL, submitted_at INTEGER NOT NULL,
    PRIMARY KEY(account_id,operation_id),
    FOREIGN KEY(account_id,operation_id) REFERENCES operations(account_id,id),
    FOREIGN KEY(revision_id,account_id,item_id) REFERENCES pr_revisions(id,account_id,item_id)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER merge_immutable_payload BEFORE UPDATE OF snapshot,projection,payload_version ON operations
WHEN EXISTS(SELECT 1 FROM merge_intents m WHERE m.account_id=OLD.account_id AND m.operation_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'submitted merge intent is immutable'); END;

DROP VIEW mutation_families;
CREATE VIEW mutation_families AS
    SELECT account_id,operation_id,'issue' AS family FROM write_intents
    UNION ALL SELECT account_id,operation_id,'review' FROM review_intents
    UNION ALL SELECT account_id,operation_id,'merge' FROM merge_intents;
CREATE VIEW pr_mutation_targets AS
    SELECT account_id,operation_id,revision_id,pr_api_id FROM review_intents
    UNION ALL SELECT account_id,operation_id,revision_id,pr_api_id FROM merge_intents;

CREATE TABLE merge_attempts (
    account_id INTEGER NOT NULL,operation_id TEXT NOT NULL,attempt INTEGER NOT NULL,
    route TEXT NOT NULL CHECK(json_valid(route)),
    observation TEXT NOT NULL CHECK(json_valid(observation)),
    decision TEXT NOT NULL,observed_at INTEGER NOT NULL,
    PRIMARY KEY(account_id,operation_id,attempt),
    FOREIGN KEY(account_id,operation_id,attempt) REFERENCES operation_attempts(account_id,operation_id,attempt)
) STRICT, WITHOUT ROWID;
CREATE TABLE merge_acceptances (
    account_id INTEGER NOT NULL,operation_id TEXT NOT NULL,attempt INTEGER NOT NULL,
    request_id TEXT NOT NULL,merge_oid TEXT,
    PRIMARY KEY(account_id,operation_id,attempt),
    FOREIGN KEY(account_id,operation_id,attempt) REFERENCES merge_attempts(account_id,operation_id,attempt)
) STRICT, WITHOUT ROWID;
CREATE TABLE confirmed_merges (
    account_id INTEGER NOT NULL,operation_id TEXT NOT NULL,item_id INTEGER NOT NULL,
    repository_id INTEGER NOT NULL,head TEXT NOT NULL,merge_oid TEXT NOT NULL,
    own_action INTEGER NOT NULL CHECK(own_action IN (0,1)),confirmed_at INTEGER NOT NULL,
    PRIMARY KEY(account_id,operation_id),
    FOREIGN KEY(account_id,operation_id) REFERENCES operations(account_id,id),
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;
