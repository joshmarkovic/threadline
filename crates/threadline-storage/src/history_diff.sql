CREATE TABLE history_diffs (
    id INTEGER PRIMARY KEY,account_id INTEGER NOT NULL,repository_id INTEGER NOT NULL,key TEXT NOT NULL CHECK(json_valid(key)),
    generation INTEGER NOT NULL DEFAULT 1,cursor TEXT,state TEXT NOT NULL DEFAULT 'new'
        CHECK(state IN ('new','partial','complete','limited')),fetched_at INTEGER,effective_base TEXT,
    UNIQUE(repository_id,key),FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id)
) STRICT;
CREATE TABLE history_diff_files (
    diff_id INTEGER NOT NULL REFERENCES history_diffs(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,path TEXT NOT NULL,record TEXT NOT NULL CHECK(json_valid(record)),
    PRIMARY KEY(diff_id,ordinal),UNIQUE(diff_id,path)
) STRICT, WITHOUT ROWID;
CREATE TABLE history_diff_cursors (
    diff_id INTEGER NOT NULL REFERENCES history_diffs(id) ON DELETE CASCADE,
    cursor TEXT NOT NULL,PRIMARY KEY(diff_id,cursor)
) STRICT, WITHOUT ROWID;
