CREATE TABLE review_intents (
    account_id INTEGER NOT NULL,
    operation_id TEXT NOT NULL,
    item_id INTEGER NOT NULL,
    revision_id INTEGER NOT NULL,
    pr_api_id TEXT NOT NULL,
    submitted_at INTEGER NOT NULL,
    PRIMARY KEY(account_id,operation_id),
    FOREIGN KEY(account_id,operation_id) REFERENCES operations(account_id,id),
    FOREIGN KEY(revision_id,account_id,item_id) REFERENCES pr_revisions(id,account_id,item_id)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER review_immutable_payload BEFORE UPDATE OF snapshot,projection,payload_version ON operations
WHEN EXISTS(SELECT 1 FROM review_intents w WHERE w.account_id=OLD.account_id AND w.operation_id=OLD.id)
BEGIN SELECT RAISE(ABORT,'submitted review intent is immutable'); END;

CREATE VIEW mutation_families AS
    SELECT account_id,operation_id,'issue' family FROM write_intents
    UNION ALL SELECT account_id,operation_id,'review' FROM review_intents;

CREATE TABLE review_attempts (
    account_id INTEGER NOT NULL,operation_id TEXT NOT NULL,attempt INTEGER NOT NULL,
    observed_at INTEGER NOT NULL,credential_generation INTEGER NOT NULL,
    route TEXT NOT NULL CHECK(json_valid(route)), step TEXT NOT NULL CHECK(json_valid(step)),
    observed TEXT NOT NULL CHECK(json_valid(observed)), outcome TEXT NOT NULL,
    PRIMARY KEY(account_id,operation_id,attempt),
    FOREIGN KEY(account_id,operation_id,attempt) REFERENCES operation_attempts(account_id,operation_id,attempt)
) STRICT, WITHOUT ROWID;

CREATE TABLE review_reanchors (
    account_id INTEGER NOT NULL,draft_id TEXT NOT NULL,revision INTEGER NOT NULL,
    item_id INTEGER NOT NULL,previous_base TEXT NOT NULL,previous_anchor TEXT NOT NULL,previous_body TEXT NOT NULL,
    PRIMARY KEY(account_id,draft_id,revision),
    FOREIGN KEY(account_id,draft_id) REFERENCES drafts(account_id,id) ON DELETE CASCADE,
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE review_anchor_proofs (
    account_id INTEGER NOT NULL,draft_id TEXT NOT NULL,item_id INTEGER NOT NULL,
    revision_id INTEGER NOT NULL,anchor TEXT NOT NULL CHECK(json_valid(anchor)),
    PRIMARY KEY(account_id,draft_id,revision_id,anchor),
    FOREIGN KEY(account_id,draft_id) REFERENCES drafts(account_id,id) ON DELETE CASCADE,
    FOREIGN KEY(revision_id,account_id,item_id) REFERENCES pr_revisions(id,account_id,item_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE review_accepted_ids (
    account_id INTEGER NOT NULL,operation_id TEXT NOT NULL,attempt INTEGER NOT NULL,
    remote_id TEXT NOT NULL,request_id TEXT NOT NULL,
    PRIMARY KEY(account_id,operation_id,attempt),
    FOREIGN KEY(account_id,operation_id,attempt) REFERENCES review_attempts(account_id,operation_id,attempt)
) STRICT, WITHOUT ROWID;

CREATE TABLE review_activity (
    account_id INTEGER NOT NULL,operation_id TEXT NOT NULL,step TEXT NOT NULL,item_id INTEGER NOT NULL,
    head TEXT NOT NULL,remote_id TEXT NOT NULL,confirmed_at INTEGER NOT NULL,
    PRIMARY KEY(account_id,operation_id,step),
    FOREIGN KEY(account_id,operation_id,step) REFERENCES step_receipts(account_id,operation_id,step),
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id)
) STRICT, WITHOUT ROWID;
