-- Restore the PAT-only credential constraint and remove the schema 19 cleanup journal.
BEGIN;
ALTER TABLE github_access_profiles ADD COLUMN original_kind INTEGER NOT NULL
    DEFAULT 0 CHECK(original_kind IN (0,1));
UPDATE github_access_profiles SET original_kind=credential_kind;
ALTER TABLE github_access_profiles DROP COLUMN credential_kind;
ALTER TABLE github_access_profiles RENAME COLUMN original_kind TO credential_kind;
DROP TABLE retired_github_pat_refs;
PRAGMA user_version=18;
COMMIT;
