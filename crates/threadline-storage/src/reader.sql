CREATE TABLE reader_facets (
    item_id INTEGER NOT NULL REFERENCES items(id),
    category TEXT NOT NULL CHECK(category IN ('author','label','assignee','reviewer','participant','mention')),
    observed_at INTEGER NOT NULL,
    PRIMARY KEY(item_id, category)
) STRICT, WITHOUT ROWID;
CREATE TABLE reader_facet_values (
    item_id INTEGER NOT NULL, category TEXT NOT NULL, value TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY(item_id,category,value),
    FOREIGN KEY(item_id,category) REFERENCES reader_facets(item_id,category) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX reader_facet_lookup ON reader_facet_values(category,value,item_id);

CREATE TABLE reader_marks (
    item_id INTEGER PRIMARY KEY REFERENCES items(id),
    pinned INTEGER NOT NULL DEFAULT 0 CHECK(pinned IN (0,1)),
    opened_at INTEGER
) STRICT;
CREATE TABLE reader_saved_views (
    account_id INTEGER NOT NULL REFERENCES accounts(id),name TEXT NOT NULL,
    filters TEXT NOT NULL CHECK(json_valid(filters)),PRIMARY KEY(account_id,name)
) STRICT, WITHOUT ROWID;

CREATE TABLE issue_downloads (
    item_id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL,
    kind TEXT NOT NULL DEFAULT 'issue' CHECK(kind='issue'),
    state TEXT NOT NULL CHECK(state IN ('pending','active','complete','cancelled')),
    generation INTEGER NOT NULL CHECK(generation>0),
    requested_at INTEGER NOT NULL,
    FOREIGN KEY(item_id,account_id,kind) REFERENCES items(id,account_id,kind)
) STRICT;
CREATE INDEX issue_download_account ON issue_downloads(account_id,state,item_id);

-- Map the existing two-part record key to the integer rowid required by FTS5.
CREATE TABLE reader_document_keys (
    id INTEGER PRIMARY KEY,
    resource_id INTEGER NOT NULL REFERENCES cached_resources(id) ON DELETE CASCADE,
    record_key TEXT NOT NULL,
    UNIQUE(resource_id,record_key)
) STRICT;
CREATE VIEW reader_document_content AS
    SELECT k.id,r.body FROM reader_document_keys k JOIN cached_resource_records r
    ON r.resource_id=k.resource_id AND r.record_key=k.record_key;
CREATE VIRTUAL TABLE conversation_search USING fts5(
    body, content='reader_document_content', content_rowid='id', tokenize='unicode61'
);
CREATE TRIGGER conversation_insert AFTER INSERT ON cached_resource_records
WHEN EXISTS(SELECT 1 FROM cached_resources WHERE id=NEW.resource_id AND item_id IS NOT NULL
    AND family IN ('comments','reviews','review-threads')) BEGIN
    INSERT INTO reader_document_keys(resource_id,record_key) VALUES(NEW.resource_id,NEW.record_key);
    INSERT INTO conversation_search(rowid,body) SELECT id,NEW.body FROM reader_document_keys
        WHERE resource_id=NEW.resource_id AND record_key=NEW.record_key;
END;
CREATE TRIGGER conversation_update AFTER UPDATE OF body ON cached_resource_records WHEN OLD.body<>NEW.body BEGIN
    INSERT INTO conversation_search(conversation_search,rowid,body)
        SELECT 'delete',id,OLD.body FROM reader_document_keys WHERE resource_id=OLD.resource_id AND record_key=OLD.record_key;
    INSERT INTO conversation_search(rowid,body) SELECT id,NEW.body FROM reader_document_keys
        WHERE resource_id=NEW.resource_id AND record_key=NEW.record_key;
END;
CREATE TRIGGER conversation_delete BEFORE DELETE ON cached_resource_records BEGIN
    INSERT INTO conversation_search(conversation_search,rowid,body)
        SELECT 'delete',id,OLD.body FROM reader_document_keys WHERE resource_id=OLD.resource_id AND record_key=OLD.record_key;
    DELETE FROM reader_document_keys WHERE resource_id=OLD.resource_id AND record_key=OLD.record_key;
END;
INSERT INTO reader_document_keys(resource_id,record_key)
SELECT r.resource_id,r.record_key FROM cached_resource_records r JOIN cached_resources c ON c.id=r.resource_id
WHERE c.item_id IS NOT NULL AND c.family IN ('comments','reviews','review-threads');
INSERT INTO conversation_search(conversation_search) VALUES('rebuild');

CREATE VIEW reader_rows AS
SELECT i.id,i.account_id,i.repository_id,r.owner,r.name,i.number,i.kind,i.title,i.closed,p.merged,
    coalesce(m.pinned,0) AS pinned,m.opened_at,
    max(coalesce(i.title_at,i.body_at,i.closed_at),coalesce(i.body_at,i.title_at,i.closed_at),
        coalesce(i.closed_at,i.title_at,i.body_at)) AS updated_at
FROM items i JOIN repositories r ON r.id=i.repository_id
LEFT JOIN pull_request_details p ON p.item_id=i.id LEFT JOIN reader_marks m ON m.item_id=i.id;

CREATE TABLE reader_closed_tabs (
    sequence INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, item_id INTEGER NOT NULL,
    tab_id TEXT NOT NULL,pane_index INTEGER NOT NULL,tab_index INTEGER NOT NULL,
    view TEXT NOT NULL CHECK(json_valid(view)), UNIQUE(account_id,tab_id),
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id)
) STRICT;
CREATE INDEX reader_last_closed ON reader_closed_tabs(account_id,sequence DESC);

CREATE TABLE reader_comment_metadata (
    resource_id INTEGER NOT NULL,record_key TEXT NOT NULL,
    node_id TEXT,author TEXT,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,
    PRIMARY KEY(resource_id,record_key),
    FOREIGN KEY(resource_id,record_key) REFERENCES cached_resource_records(resource_id,record_key) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE TABLE reader_comment_staging (
    resource_id INTEGER NOT NULL REFERENCES cached_resources(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,record_key TEXT NOT NULL,node_id TEXT,author TEXT,
    created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,
    PRIMARY KEY(resource_id,generation,record_key)
) STRICT, WITHOUT ROWID;
CREATE INDEX reader_comment_order ON reader_comment_metadata(resource_id,created_at,record_key);

CREATE TABLE cached_images (
    account_id INTEGER NOT NULL, repository_id INTEGER NOT NULL, stable_url TEXT NOT NULL,
    mime TEXT NOT NULL, bytes BLOB NOT NULL CHECK(length(bytes) BETWEEN 1 AND 8388608),
    stored_at INTEGER NOT NULL, PRIMARY KEY(account_id,repository_id,stable_url),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX cached_images_age ON cached_images(account_id,stored_at);
