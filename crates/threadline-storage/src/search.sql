CREATE VIRTUAL TABLE item_search USING fts5 (
    title, body, content = 'items', content_rowid = 'id', tokenize = 'unicode61'
);

CREATE TRIGGER item_search_insert AFTER INSERT ON items BEGIN
    INSERT INTO item_search (rowid, title, body) VALUES (new.id, new.title, new.body);
END;

CREATE TRIGGER item_search_delete AFTER DELETE ON items BEGIN
    INSERT INTO item_search (item_search, rowid, title, body)
        VALUES ('delete', old.id, old.title, old.body);
END;

CREATE TRIGGER item_search_update AFTER UPDATE OF title, body ON items
WHEN old.title IS NOT new.title OR old.body IS NOT new.body BEGIN
    INSERT INTO item_search (item_search, rowid, title, body)
        VALUES ('delete', old.id, old.title, old.body);
    INSERT INTO item_search (rowid, title, body) VALUES (new.id, new.title, new.body);
END;

INSERT INTO item_search (item_search) VALUES ('rebuild');
