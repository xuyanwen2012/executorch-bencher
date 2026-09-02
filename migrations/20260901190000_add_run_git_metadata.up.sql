-- Optional git commit metadata. All three are nullable: runs recorded
-- before this migration, and runs recorded by a collector that does not
-- capture them, read back as NULL.
ALTER TABLE runs ADD COLUMN git_branch TEXT;
ALTER TABLE runs ADD COLUMN git_commit_timestamp TEXT;
ALTER TABLE runs ADD COLUMN git_commit_subject TEXT;
