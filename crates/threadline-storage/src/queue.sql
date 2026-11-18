CREATE TABLE workspaces (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id), layout TEXT NOT NULL CHECK(json_valid(layout))
);
CREATE TABLE pr_contexts (
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    pr_id INTEGER NOT NULL REFERENCES items(id),
    mode TEXT NOT NULL CHECK(mode IN ('pr','issue')),
    issue_id INTEGER REFERENCES items(id), body_revision TEXT NOT NULL, provenance TEXT NOT NULL,
    context_ratio REAL NOT NULL CHECK(context_ratio > 0 AND context_ratio < 1),
    PRIMARY KEY(account_id, pr_id),
    FOREIGN KEY(pr_id,account_id) REFERENCES items(id,account_id),
    FOREIGN KEY(issue_id,account_id) REFERENCES items(id,account_id),
    CHECK(mode = 'pr' OR (issue_id IS NOT NULL AND length(body_revision)>0 AND length(provenance)>0))
);
CREATE TABLE local_preferences (
    account_id INTEGER NOT NULL REFERENCES accounts(id), name TEXT NOT NULL, value TEXT NOT NULL,
    PRIMARY KEY(account_id, name)
);
CREATE TABLE dispatch_profiles (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id), authorized INTEGER NOT NULL DEFAULT 0,
    offline INTEGER NOT NULL DEFAULT 0, outbox_paused INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE active_profile (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1), account_id INTEGER NOT NULL REFERENCES accounts(id)
);
CREATE TABLE restored_profiles(account_id INTEGER PRIMARY KEY REFERENCES accounts(id));
CREATE TABLE paused_repositories (
    account_id INTEGER NOT NULL REFERENCES accounts(id), repository_id INTEGER NOT NULL REFERENCES repositories(id),
    PRIMARY KEY(account_id, repository_id),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id)
);
CREATE TABLE drafts (
    account_id INTEGER NOT NULL REFERENCES accounts(id), id TEXT NOT NULL CHECK(length(id)>0),
    item_id INTEGER NOT NULL REFERENCES items(id),
    kind TEXT NOT NULL CHECK(kind IN ('issue_edit','comment','review','reply')),
    body TEXT NOT NULL, base TEXT NOT NULL, anchor TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>=1),
    editor TEXT, editor_until INTEGER,
    PRIMARY KEY(account_id,id), FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id),
    CHECK((editor IS NULL) = (editor_until IS NULL))
);
CREATE TABLE operations (
    sequence INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id), id TEXT NOT NULL CHECK(length(id)>0),
    original_repository INTEGER NOT NULL REFERENCES repositories(id), item_id INTEGER NOT NULL REFERENCES items(id),
    original_number INTEGER NOT NULL CHECK(original_number>0),
    kind TEXT NOT NULL CHECK(kind IN ('issue_edit','comment','review','reply','resolve_thread','merge')),
    payload_version INTEGER NOT NULL, snapshot TEXT NOT NULL CHECK(json_valid(snapshot)),
    projection TEXT NOT NULL, draft_id TEXT, draft_revision INTEGER,
    state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','validating','sending','unknown','waiting','attention','confirmed','cancelled')),
    reason TEXT, retry_at INTEGER NOT NULL DEFAULT 0, cancel_requested INTEGER NOT NULL DEFAULT 0,
    attempt INTEGER NOT NULL DEFAULT 0, lease_until INTEGER,
    UNIQUE(account_id,id), UNIQUE(account_id,draft_id,draft_revision),
    FOREIGN KEY(original_repository,account_id) REFERENCES repositories(id,account_id),
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id),
    CHECK((draft_id IS NULL) = (draft_revision IS NULL))
);
CREATE INDEX queue_by_target ON operations(account_id,item_id,sequence,state);
CREATE INDEX queue_ready ON operations(account_id,state,retry_at,sequence);
CREATE TRIGGER immutable_operation_target BEFORE UPDATE OF account_id,id,original_repository,original_number,item_id,kind ON operations
BEGIN
    SELECT RAISE(ABORT,'submitted operation target is immutable');
END;
CREATE TABLE restored_operations (
    recovery_id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, operation_id TEXT NOT NULL,
    snapshot_state TEXT NOT NULL, snapshot_reason TEXT, decision TEXT, evidence TEXT,
    FOREIGN KEY(account_id,operation_id) REFERENCES operations(account_id,id),
    CHECK((decision IS NULL)=(evidence IS NULL))
);
CREATE UNIQUE INDEX open_restore_check ON restored_operations(account_id, operation_id) WHERE decision IS NULL;
CREATE TABLE reviewed_operation_locators (
    account_id INTEGER NOT NULL, operation_id TEXT NOT NULL, repository_id INTEGER NOT NULL,
    number INTEGER NOT NULL CHECK(number>0), evidence TEXT NOT NULL,
    FOREIGN KEY(account_id,operation_id) REFERENCES operations(account_id,id),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id),
    PRIMARY KEY(account_id,operation_id)
);
CREATE TABLE operation_dependencies (
    account_id INTEGER NOT NULL, dependent TEXT NOT NULL, predecessor TEXT NOT NULL,
    PRIMARY KEY(account_id,dependent,predecessor),
    FOREIGN KEY(account_id,dependent) REFERENCES operations(account_id,id),
    FOREIGN KEY(account_id,predecessor) REFERENCES operations(account_id,id),
    CHECK(dependent != predecessor)
);
CREATE INDEX queue_dependents ON operation_dependencies(account_id,predecessor);
CREATE TABLE operation_attempts (
    account_id INTEGER NOT NULL, operation_id TEXT NOT NULL, attempt INTEGER NOT NULL,
    claimed_at INTEGER NOT NULL, claimed_repository INTEGER NOT NULL, claimed_number INTEGER NOT NULL,
    sending_at INTEGER, step TEXT, outcome TEXT, evidence TEXT,
    PRIMARY KEY(account_id,operation_id,attempt),
    FOREIGN KEY(account_id,operation_id) REFERENCES operations(account_id,id),
    FOREIGN KEY(claimed_repository,account_id) REFERENCES repositories(id,account_id)
);
CREATE TABLE step_receipts (
    account_id INTEGER NOT NULL, operation_id TEXT NOT NULL, step TEXT NOT NULL,
    object_kind TEXT NOT NULL, remote_id TEXT NOT NULL, evidence TEXT NOT NULL,
    PRIMARY KEY(account_id,operation_id,step),
    FOREIGN KEY(account_id,operation_id) REFERENCES operations(account_id,id)
);
CREATE TABLE attempt_events (
    event_id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL, operation_id TEXT NOT NULL, attempt INTEGER NOT NULL,
    outcome TEXT NOT NULL, evidence TEXT NOT NULL,
    FOREIGN KEY(account_id,operation_id,attempt) REFERENCES operation_attempts(account_id,operation_id,attempt)
);
CREATE INDEX attempt_history ON attempt_events(account_id,operation_id,event_id);
CREATE TABLE pr_reference_sets (
    account_id INTEGER NOT NULL REFERENCES accounts(id), pr_id INTEGER NOT NULL REFERENCES items(id),
    body_revision TEXT NOT NULL, PRIMARY KEY(account_id,pr_id),
    FOREIGN KEY(pr_id,account_id) REFERENCES items(id,account_id)
);
CREATE TABLE pr_reference_candidates (
    account_id INTEGER NOT NULL, pr_id INTEGER NOT NULL, position INTEGER NOT NULL,
    raw_reference TEXT NOT NULL, provenance TEXT NOT NULL, verified_issue INTEGER REFERENCES items(id),
    verified_at INTEGER,
    PRIMARY KEY(account_id,pr_id,position),
    FOREIGN KEY(account_id,pr_id) REFERENCES pr_reference_sets(account_id,pr_id),
    FOREIGN KEY(verified_issue,account_id) REFERENCES items(id,account_id),
    CHECK((verified_issue IS NULL)=(verified_at IS NULL))
);
CREATE VIEW operation_gates AS
SELECT q.account_id,q.id,
CASE
 WHEN EXISTS(SELECT 1 FROM restored_profiles p WHERE p.account_id=q.account_id) THEN 'restored_backup'
 WHEN EXISTS(SELECT 1 FROM restored_operations s WHERE s.account_id=q.account_id AND s.operation_id=q.id AND s.decision IS NULL) THEN 'restored_operation'
 WHEN NOT EXISTS(SELECT 1 FROM active_profile a WHERE a.account_id=q.account_id) THEN 'inactive_profile'
 WHEN NOT EXISTS(SELECT 1 FROM dispatch_profiles p WHERE p.account_id=q.account_id AND p.authorized=1) THEN 'signed_out'
 WHEN EXISTS(SELECT 1 FROM dispatch_profiles p WHERE p.account_id=q.account_id AND p.offline=1) THEN 'offline'
 WHEN EXISTS(SELECT 1 FROM dispatch_profiles p WHERE p.account_id=q.account_id AND p.outbox_paused=1) THEN 'paused'
 WHEN EXISTS(SELECT 1 FROM paused_repositories p JOIN items i ON i.id=q.item_id WHERE
         p.account_id=q.account_id AND p.repository_id IN(q.original_repository,i.repository_id)) THEN 'repository_scope'
 WHEN EXISTS(SELECT 1 FROM items i WHERE i.id=q.item_id AND (i.repository_id<>q.original_repository OR i.number<>q.original_number)
         AND NOT EXISTS(SELECT 1 FROM reviewed_operation_locators v WHERE v.account_id=q.account_id AND v.operation_id=q.id
              AND v.repository_id=i.repository_id AND v.number=i.number)) THEN 'locator_changed'
 WHEN EXISTS(SELECT 1 FROM operation_dependencies d JOIN operations parent ON parent.id=d.predecessor AND parent.account_id=d.account_id
         WHERE d.account_id=q.account_id AND d.dependent=q.id AND parent.state<>'confirmed') THEN 'dependency'
 WHEN EXISTS(SELECT 1 FROM operations older WHERE older.account_id=q.account_id AND older.item_id=q.item_id
         AND older.sequence<q.sequence AND older.state NOT IN ('confirmed','cancelled')) THEN 'target_order'
 ELSE NULL END AS hold
FROM operations q;
