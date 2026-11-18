CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    host TEXT NOT NULL COLLATE NOCASE CHECK (length(host) > 0),
    provider_user_id TEXT NOT NULL CHECK (length(provider_user_id) > 0),
    UNIQUE (host, provider_user_id)
) STRICT;

CREATE TABLE repositories (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id),
    provider_repository_id TEXT NOT NULL CHECK (length(provider_repository_id) > 0),
    owner TEXT NOT NULL COLLATE NOCASE,
    name TEXT NOT NULL COLLATE NOCASE,
    UNIQUE (account_id, provider_repository_id),
    UNIQUE (account_id, owner, name),
    UNIQUE (id, account_id)
) STRICT;

CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    repository_id INTEGER NOT NULL,
    account_id INTEGER NOT NULL,
    number INTEGER NOT NULL CHECK (number > 0),
    kind TEXT NOT NULL DEFAULT 'unknown' CHECK (kind IN ('unknown', 'issue', 'pull_request')),
    issue_api_id TEXT,
    issue_node_id TEXT,
    title TEXT,
    title_at INTEGER,
    body TEXT,
    body_at INTEGER,
    closed INTEGER CHECK (closed IN (0, 1)),
    closed_at INTEGER,
    FOREIGN KEY (repository_id, account_id) REFERENCES repositories(id, account_id),
    UNIQUE (repository_id, number),
    UNIQUE (account_id, issue_api_id),
    UNIQUE (account_id, issue_node_id),
    UNIQUE (id, account_id),
    UNIQUE (id, account_id, kind)
) STRICT;

CREATE TABLE pull_request_details (
    item_id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    kind TEXT NOT NULL DEFAULT 'pull_request' CHECK (kind = 'pull_request'),
    pull_request_api_id TEXT NOT NULL,
    pull_request_node_id TEXT,
    merged INTEGER CHECK (merged IN (0, 1)),
    merged_at INTEGER,
    base_oid TEXT,
    base_oid_at INTEGER,
    head_oid TEXT,
    head_oid_at INTEGER,
    merge_oid TEXT,
    merge_oid_at INTEGER,
    FOREIGN KEY (item_id, account_id, kind) REFERENCES items(id, account_id, kind),
    UNIQUE (account_id, pull_request_api_id),
    UNIQUE (account_id, pull_request_node_id)
) STRICT;

CREATE TABLE item_aliases (
    repository_id INTEGER NOT NULL,
    account_id INTEGER NOT NULL,
    number INTEGER NOT NULL CHECK (number > 0),
    item_id INTEGER NOT NULL,
    PRIMARY KEY (repository_id, number),
    FOREIGN KEY (repository_id, account_id) REFERENCES repositories(id, account_id),
    FOREIGN KEY (item_id, account_id) REFERENCES items(id, account_id)
) STRICT, WITHOUT ROWID;
