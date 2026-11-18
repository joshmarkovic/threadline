CREATE TABLE history_details (
    repository_id INTEGER NOT NULL,oid TEXT NOT NULL,message TEXT NOT NULL,
    author TEXT,authored_at INTEGER,
    PRIMARY KEY(repository_id,oid),
    FOREIGN KEY(repository_id,oid) REFERENCES cached_commits(repository_id,oid) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE TABLE history_series (
    id INTEGER PRIMARY KEY,account_id INTEGER NOT NULL,repository_id INTEGER NOT NULL,tip TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 1,cursor TEXT,state TEXT NOT NULL DEFAULT 'new'
      CHECK(state IN ('new','partial','complete','limited')),
    row_count INTEGER NOT NULL DEFAULT 0,frontier TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(frontier)),
    fetched_at INTEGER,UNIQUE(repository_id,tip),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id)
) STRICT;
CREATE TABLE history_rows (
    series_id INTEGER NOT NULL REFERENCES history_series(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,oid TEXT NOT NULL,lane INTEGER NOT NULL,
    parents TEXT NOT NULL CHECK(json_valid(parents)),
    PRIMARY KEY(series_id,ordinal),UNIQUE(series_id,oid)
) STRICT, WITHOUT ROWID;
CREATE TABLE history_waiting (
    series_id INTEGER NOT NULL REFERENCES history_series(id) ON DELETE CASCADE,
    oid TEXT NOT NULL,PRIMARY KEY(series_id,oid)
) STRICT, WITHOUT ROWID;
CREATE TABLE history_page_cursors (
    series_id INTEGER NOT NULL REFERENCES history_series(id) ON DELETE CASCADE,
    cursor TEXT NOT NULL,PRIMARY KEY(series_id,cursor)
) STRICT, WITHOUT ROWID;
CREATE TABLE history_views (
    account_id INTEGER NOT NULL,repo INTEGER NOT NULL,view_id TEXT NOT NULL,
    generation INTEGER NOT NULL,state TEXT NOT NULL CHECK(json_valid(state)),
    PRIMARY KEY(account_id,repo,view_id),
    FOREIGN KEY(repo,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE history_monitoring (
    account_id INTEGER NOT NULL,repo INTEGER NOT NULL,settings TEXT NOT NULL CHECK(json_valid(settings)),
    PRIMARY KEY(account_id,repo),FOREIGN KEY(repo,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE history_ref_baselines (
    account_id INTEGER NOT NULL,repo INTEGER NOT NULL,ref_key TEXT NOT NULL,tip TEXT,observed_at INTEGER NOT NULL,
    PRIMARY KEY(account_id,repo,ref_key),FOREIGN KEY(repo,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE history_ref_moves (
    id INTEGER PRIMARY KEY,account_id INTEGER NOT NULL,repo INTEGER NOT NULL,ref_key TEXT NOT NULL,
    old_tip TEXT NOT NULL,new_tip TEXT NOT NULL,observed_at INTEGER NOT NULL,
    outcome TEXT NOT NULL DEFAULT 'unknown',last_checked INTEGER NOT NULL DEFAULT 0,UNIQUE(account_id,repo,ref_key,observed_at),
    FOREIGN KEY(repo,account_id) REFERENCES repositories(id,account_id)
) STRICT;
CREATE INDEX history_pending_moves ON history_ref_moves(repo,outcome,id);
CREATE TABLE history_ref_tips (
    account_id INTEGER NOT NULL,repo INTEGER NOT NULL,ref_key TEXT NOT NULL,tip TEXT NOT NULL,
    last_observed_at INTEGER NOT NULL,PRIMARY KEY(account_id,repo,ref_key,tip),
    FOREIGN KEY(repo,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX cached_ref_target_labels ON cached_refs(repository_id,oid,name) WHERE oid IS NOT NULL;
