CREATE TABLE cached_resources (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    repository_id INTEGER,
    item_id INTEGER,
    family TEXT NOT NULL,
    coverage INTEGER NOT NULL DEFAULT 0 CHECK (coverage BETWEEN 0 AND 2),
    fetch INTEGER NOT NULL DEFAULT 0 CHECK (fetch BETWEEN 0 AND 3),
    error INTEGER CHECK (error BETWEEN 1 AND 9),
    generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
    walk_mode INTEGER NOT NULL DEFAULT 0 CHECK (walk_mode IN (0, 1)),
    continuation TEXT,
    has_page INTEGER NOT NULL DEFAULT 0 CHECK (has_page IN (0, 1)),
    last_page_at_ms INTEGER,
    last_fetch_at_ms INTEGER,
    last_validation_at_ms INTEGER,
    etag TEXT,
    last_modified TEXT,
    revision TEXT,
    FOREIGN KEY (repository_id, account_id) REFERENCES repositories(id, account_id),
    FOREIGN KEY (item_id, account_id) REFERENCES items(id, account_id),
    CHECK ((repository_id IS NULL) != (item_id IS NULL)),
    CHECK ((fetch IN (2, 3)) = (error IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX cached_resource_repository ON cached_resources(repository_id, family)
    WHERE repository_id IS NOT NULL;
CREATE UNIQUE INDEX cached_resource_item ON cached_resources(item_id, family)
    WHERE item_id IS NOT NULL;
CREATE INDEX cached_resource_account ON cached_resources(account_id);

CREATE TABLE cached_resource_records (
    resource_id INTEGER NOT NULL REFERENCES cached_resources(id) ON DELETE CASCADE,
    record_key TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (resource_id, record_key)
) WITHOUT ROWID, STRICT;

CREATE TABLE cached_resource_pending (
    resource_id INTEGER NOT NULL REFERENCES cached_resources(id) ON DELETE CASCADE,
    record_key TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (resource_id, record_key)
) WITHOUT ROWID, STRICT;

CREATE TABLE cached_resource_cursors (
    resource_id INTEGER NOT NULL REFERENCES cached_resources(id) ON DELETE CASCADE,
    cursor TEXT NOT NULL,
    PRIMARY KEY (resource_id, cursor)
) WITHOUT ROWID, STRICT;

CREATE TABLE cached_comparisons (
    id INTEGER PRIMARY KEY,
    repository_id INTEGER NOT NULL REFERENCES repositories(id),
    base_commit TEXT NOT NULL,
    head_commit TEXT NOT NULL,
    compare_mode INTEGER NOT NULL CHECK (compare_mode IN (0, 1)),
    ignore_whitespace INTEGER NOT NULL CHECK (ignore_whitespace IN (0, 1)),
    context_lines INTEGER NOT NULL CHECK (context_lines BETWEEN 0 AND 65535),
    UNIQUE (repository_id, base_commit, head_commit, compare_mode, ignore_whitespace, context_lines)
) STRICT;
CREATE TABLE cached_comparison_files (
    comparison_id INTEGER NOT NULL REFERENCES cached_comparisons(id) ON DELETE CASCADE,
    file_index INTEGER NOT NULL CHECK (file_index >= 0),
    path_label TEXT NOT NULL,
    previous_path_label TEXT,
    patch TEXT,
    PRIMARY KEY (comparison_id, file_index)
) WITHOUT ROWID, STRICT;

CREATE TABLE cached_commits (
    repository_id INTEGER NOT NULL REFERENCES repositories(id),
    oid TEXT NOT NULL,
    title TEXT NOT NULL,
    committed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (repository_id, oid)
) WITHOUT ROWID, STRICT;
CREATE TABLE cached_commit_parents (
    repository_id INTEGER NOT NULL,
    child_oid TEXT NOT NULL,
    parent_index INTEGER NOT NULL CHECK (parent_index >= 0),
    parent_oid TEXT NOT NULL,
    FOREIGN KEY (repository_id, child_oid) REFERENCES cached_commits(repository_id, oid) ON DELETE CASCADE,
    PRIMARY KEY (repository_id, child_oid, parent_index),
    UNIQUE (repository_id, child_oid, parent_oid)
) WITHOUT ROWID, STRICT;
CREATE INDEX cached_commit_children ON cached_commit_parents(repository_id, parent_oid);
CREATE TABLE cached_refs (
    repository_id INTEGER NOT NULL REFERENCES repositories(id),
    name TEXT NOT NULL,
    oid TEXT,
    observed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (repository_id, name)
) WITHOUT ROWID, STRICT;

CREATE TABLE cached_revision_states (
    item_id INTEGER NOT NULL REFERENCES items(id),
    family INTEGER NOT NULL CHECK (family IN (0, 1)),
    oid TEXT NOT NULL,
    payload TEXT NOT NULL,
    observed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (item_id, family, oid)
) WITHOUT ROWID, STRICT;

CREATE TABLE sync_jobs (
    repository_id INTEGER NOT NULL REFERENCES repositories(id),
    family TEXT NOT NULL,
    phase INTEGER NOT NULL CHECK (phase BETWEEN 0 AND 3),
    resume_after_ms INTEGER,
    updated_at_ms INTEGER NOT NULL,
    reason INTEGER CHECK (reason BETWEEN 1 AND 9),
    checkpoint TEXT,
    overlap_from_ms INTEGER,
    PRIMARY KEY (repository_id, family)
) WITHOUT ROWID, STRICT;

CREATE TABLE activity_baselines (
    repository_id INTEGER NOT NULL REFERENCES repositories(id),
    stream_key TEXT NOT NULL,
    checkpoint TEXT NOT NULL,
    observed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (repository_id, stream_key)
) WITHOUT ROWID, STRICT;

CREATE TABLE activity_events (
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    event_key TEXT NOT NULL,
    repository_id INTEGER NOT NULL,
    item_id INTEGER,
    event_kind TEXT NOT NULL,
    revision TEXT,
    occurred_at_ms INTEGER,
    observed_at_ms INTEGER NOT NULL,
    FOREIGN KEY (repository_id, account_id) REFERENCES repositories(id, account_id),
    FOREIGN KEY (item_id, account_id) REFERENCES items(id, account_id),
    PRIMARY KEY (account_id, event_key)
) WITHOUT ROWID, STRICT;

CREATE TABLE notification_delivery (
    account_id INTEGER NOT NULL,
    event_key TEXT NOT NULL,
    phase INTEGER NOT NULL CHECK (phase BETWEEN 0 AND 3),
    group_key TEXT,
    native_id TEXT,
    reason TEXT,
    updated_at_ms INTEGER NOT NULL,
    FOREIGN KEY (account_id, event_key) REFERENCES activity_events(account_id, event_key),
    PRIMARY KEY (account_id, event_key)
) WITHOUT ROWID, STRICT;
