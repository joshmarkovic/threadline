CREATE TABLE pr_revisions (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    repository_id INTEGER NOT NULL,
    item_id INTEGER NOT NULL,
    base_oid TEXT NOT NULL,
    head_oid TEXT NOT NULL,
    merge_base_oid TEXT,
    header TEXT NOT NULL CHECK(json_valid(header)),
    updated_at INTEGER NOT NULL,
    body TEXT,
    body_at INTEGER,
    UNIQUE(account_id,repository_id,item_id,base_oid,head_oid),
    UNIQUE(id,account_id,item_id),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id),
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id)
) STRICT;
CREATE INDEX pr_revision_newest ON pr_revisions(account_id,item_id,updated_at DESC,id ASC);
CREATE TABLE pr_family_resources (
    revision_id INTEGER NOT NULL REFERENCES pr_revisions(id),
    family TEXT NOT NULL,
    resource_id INTEGER NOT NULL UNIQUE REFERENCES cached_resources(id) ON DELETE CASCADE,
    PRIMARY KEY(revision_id,family)
) STRICT,WITHOUT ROWID;
CREATE TABLE pr_viewed_files (
    revision_id INTEGER NOT NULL REFERENCES pr_revisions(id),
    path TEXT NOT NULL,
    PRIMARY KEY(revision_id,path)
) STRICT,WITHOUT ROWID;
CREATE TABLE pr_downloads (
    revision_id INTEGER PRIMARY KEY REFERENCES pr_revisions(id),
    generation INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','active','complete','partial','cancelled')),
    requested_at INTEGER NOT NULL,
    include_context INTEGER NOT NULL CHECK(include_context IN (0,1)),
    include_media INTEGER NOT NULL CHECK(include_media IN (0,1))
) STRICT;
CREATE TABLE pr_reference_results (
    account_id INTEGER NOT NULL,pr_id INTEGER NOT NULL,position INTEGER NOT NULL,
    owner TEXT NOT NULL COLLATE NOCASE,repository TEXT NOT NULL COLLATE NOCASE,number INTEGER NOT NULL CHECK(number>0),
    verification TEXT NOT NULL CHECK(verification IN ('pending','verified','pull_request','missing','offline','inaccessible','failed')),
    checked_at INTEGER,
    PRIMARY KEY(account_id,pr_id,position),
    FOREIGN KEY(account_id,pr_id,position) REFERENCES pr_reference_candidates(account_id,pr_id,position) ON DELETE CASCADE
) STRICT,WITHOUT ROWID;

DROP TRIGGER conversation_insert;
DROP TRIGGER conversation_update;
DROP TRIGGER conversation_delete;
DROP VIEW reader_document_content;
CREATE VIEW reader_document_content AS
SELECT k.id,
    CASE WHEN c.family GLOB 'pr:reviews:*' OR c.family GLOB 'pr:threads:*'
         THEN json_extract(r.body,'$.search') ELSE r.body END AS body
FROM reader_document_keys k JOIN cached_resource_records r USING(resource_id,record_key)
JOIN cached_resources c ON c.id=k.resource_id;

CREATE TRIGGER conversation_insert AFTER INSERT ON cached_resource_records
WHEN EXISTS(SELECT 1 FROM cached_resources WHERE id=NEW.resource_id AND item_id IS NOT NULL
    AND (family IN ('comments','reviews','review-threads') OR family GLOB 'pr:reviews:*' OR family GLOB 'pr:threads:*')) BEGIN
    INSERT INTO reader_document_keys(resource_id,record_key) VALUES(NEW.resource_id,NEW.record_key);
    INSERT INTO conversation_search(rowid,body) SELECT d.id,d.body FROM reader_document_content d JOIN reader_document_keys k USING(id)
        WHERE k.resource_id=NEW.resource_id AND k.record_key=NEW.record_key;
END;
CREATE TRIGGER conversation_before_update BEFORE UPDATE OF body ON cached_resource_records WHEN OLD.body<>NEW.body BEGIN
    INSERT INTO conversation_search(conversation_search,rowid,body)
        SELECT 'delete',d.id,d.body FROM reader_document_content d JOIN reader_document_keys k USING(id)
        WHERE k.resource_id=OLD.resource_id AND k.record_key=OLD.record_key;
END;
CREATE TRIGGER conversation_update AFTER UPDATE OF body ON cached_resource_records WHEN OLD.body<>NEW.body BEGIN
    INSERT INTO conversation_search(rowid,body) SELECT d.id,d.body FROM reader_document_content d JOIN reader_document_keys k USING(id)
        WHERE k.resource_id=NEW.resource_id AND k.record_key=NEW.record_key;
END;
CREATE TRIGGER conversation_delete BEFORE DELETE ON cached_resource_records BEGIN
    INSERT INTO conversation_search(conversation_search,rowid,body)
        SELECT 'delete',d.id,d.body FROM reader_document_content d JOIN reader_document_keys k USING(id)
        WHERE k.resource_id=OLD.resource_id AND k.record_key=OLD.record_key;
    DELETE FROM reader_document_keys WHERE resource_id=OLD.resource_id AND record_key=OLD.record_key;
END;

CREATE VIEW pr_pinned_items AS
SELECT DISTINCT r.account_id,r.item_id FROM pr_revisions r JOIN pr_downloads d ON d.revision_id=r.id
WHERE d.state IN ('pending','active','partial','complete');
CREATE VIEW pr_pinned_resources AS
SELECT f.resource_id FROM pr_family_resources f JOIN pr_downloads d USING(revision_id)
WHERE d.state IN ('pending','active','partial','complete') AND (f.family<>'context' OR d.include_context)
    AND (f.family<>'media' OR d.include_media)
UNION
SELECT c.id FROM cached_resources c JOIN pr_pinned_items p USING(item_id,account_id)
WHERE c.family IN ('metadata','body','comments','timeline');
CREATE VIEW pr_pinned_images AS
SELECT DISTINCT rev.account_id,rev.repository_id,json_extract(row.body,'$.record.Media.cache_key') AS stable_url
FROM pr_downloads d JOIN pr_revisions rev ON rev.id=d.revision_id
JOIN pr_family_resources f ON f.revision_id=rev.id AND f.family='media'
JOIN cached_resource_records row USING(resource_id)
WHERE d.state IN ('pending','active','partial','complete') AND d.include_media;
