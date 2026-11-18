CREATE TABLE reader_issue_fields (
    item_id INTEGER NOT NULL REFERENCES items(id),
    field TEXT NOT NULL CHECK(field IN ('created','milestone','reactions')),
    value TEXT NOT NULL CHECK(json_valid(value)),
    observed_at INTEGER NOT NULL,
    PRIMARY KEY(item_id,field)
) STRICT, WITHOUT ROWID;
