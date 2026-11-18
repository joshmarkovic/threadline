CREATE TABLE write_repository_choices (
    account_id INTEGER NOT NULL,
    repository_id INTEGER NOT NULL,
    family TEXT NOT NULL CHECK(family IN ('labels','assignees','milestones')),
    value TEXT NOT NULL CHECK(json_valid(value)),
    observed_at INTEGER NOT NULL,
    PRIMARY KEY(account_id, repository_id, family),
    FOREIGN KEY(repository_id,account_id) REFERENCES repositories(id,account_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE write_issue_milestones (
    account_id INTEGER NOT NULL,
    item_id INTEGER NOT NULL,
    number TEXT,
    observed_at INTEGER NOT NULL,
    PRIMARY KEY(account_id, item_id),
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE write_comment_authors (
    account_id INTEGER NOT NULL,
    item_id INTEGER NOT NULL,
    comment_id TEXT NOT NULL,
    author_id TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    PRIMARY KEY(account_id,item_id,comment_id),
    FOREIGN KEY(item_id,account_id) REFERENCES items(id,account_id)
) STRICT, WITHOUT ROWID;
