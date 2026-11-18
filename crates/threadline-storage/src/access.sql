CREATE TABLE github_access_profiles (
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    owner_scope TEXT NOT NULL COLLATE NOCASE CHECK(length(owner_scope) > 0),
    login TEXT NOT NULL CHECK(length(login) > 0),
    credential_ref TEXT NOT NULL UNIQUE CHECK(length(credential_ref)=32),
    credential_kind INTEGER NOT NULL CHECK(credential_kind IN (0, 1)),
    access INTEGER NOT NULL DEFAULT 0 CHECK(access BETWEEN 0 AND 11),
    discovery INTEGER NOT NULL DEFAULT 0 CHECK(discovery BETWEEN 0 AND 3),
    checked_at_ms INTEGER,
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation > 0),
    credential_generation INTEGER NOT NULL DEFAULT 1 CHECK(credential_generation > 0),
    installed_generation INTEGER,
    PRIMARY KEY(account_id, owner_scope)
) WITHOUT ROWID, STRICT;

CREATE TABLE github_repository_access (
    repository_id INTEGER NOT NULL,
    account_id INTEGER NOT NULL,
    owner_scope TEXT NOT NULL COLLATE NOCASE,
    selected INTEGER NOT NULL DEFAULT 0 CHECK(selected IN (0, 1)),
    private INTEGER CHECK(private IN (0, 1)),
    access INTEGER NOT NULL CHECK(access BETWEEN 0 AND 11),
    checked_at_ms INTEGER NOT NULL,
    last_success_at_ms INTEGER,
    verified_generation INTEGER,
    PRIMARY KEY(repository_id, account_id),
    FOREIGN KEY(repository_id, account_id) REFERENCES repositories(id, account_id),
    FOREIGN KEY(account_id, owner_scope) REFERENCES github_access_profiles(account_id, owner_scope)
) WITHOUT ROWID, STRICT;
CREATE INDEX github_repository_access_account ON github_repository_access(account_id, owner_scope);

-- Operations retain the exact owner observed when the original repository was bound.
CREATE TABLE github_operation_scopes (
    account_id INTEGER NOT NULL,
    operation_id TEXT NOT NULL,
    owner_scope TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY(account_id, operation_id),
    FOREIGN KEY(account_id, operation_id) REFERENCES operations(account_id, id)
) WITHOUT ROWID, STRICT;
INSERT INTO github_operation_scopes(account_id,operation_id,owner_scope)
    SELECT operations.account_id, operations.id, repositories.owner
    FROM operations JOIN repositories
      ON repositories.id = operations.original_repository AND repositories.account_id = operations.account_id;
CREATE TRIGGER capture_operation_owner_scope AFTER INSERT ON operations
BEGIN
    INSERT INTO github_operation_scopes(account_id,operation_id,owner_scope)
        SELECT NEW.account_id,NEW.id,owner FROM repositories
        WHERE id=NEW.original_repository AND account_id=NEW.account_id;
END;

CREATE VIEW github_dispatch_holds AS
SELECT o.account_id,o.id,CASE
    WHEN current_repo.owner <> scope.owner_scope THEN 'credential_scope_changed'
    WHEN profile.access IS NULL OR profile.access <> 1 THEN 'credential_scope_unavailable'
    WHEN original_access.selected IS NULL OR original_access.selected <> 1
      OR current_access.selected IS NULL OR current_access.selected <> 1 THEN 'repository_unselected'
    WHEN original_access.access <> 1 OR current_access.access <> 1
      OR original_access.owner_scope <> scope.owner_scope OR current_access.owner_scope <> scope.owner_scope
      OR original_access.verified_generation IS NOT profile.credential_generation
      OR current_access.verified_generation IS NOT profile.credential_generation THEN 'repository_access'
    ELSE NULL END AS hold
FROM operations o
JOIN github_operation_scopes scope ON scope.account_id=o.account_id AND scope.operation_id=o.id
JOIN items item ON item.id=o.item_id AND item.account_id=o.account_id
JOIN repositories current_repo ON current_repo.id=item.repository_id AND current_repo.account_id=o.account_id
LEFT JOIN github_access_profiles profile ON profile.account_id=o.account_id AND profile.owner_scope=scope.owner_scope
LEFT JOIN github_repository_access original_access ON original_access.repository_id=o.original_repository AND original_access.account_id=o.account_id
LEFT JOIN github_repository_access current_access ON current_access.repository_id=item.repository_id AND current_access.account_id=o.account_id
WHERE EXISTS(SELECT 1 FROM github_access_profiles enrolled WHERE enrolled.account_id=o.account_id);

DROP VIEW operation_gates;
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
 WHEN EXISTS(SELECT 1 FROM github_dispatch_holds h WHERE h.account_id=q.account_id AND h.id=q.id AND h.hold IS NOT NULL)
     THEN (SELECT h.hold FROM github_dispatch_holds h WHERE h.account_id=q.account_id AND h.id=q.id)
 ELSE NULL END AS hold
FROM operations q;
