-- Anchor for the raw-upload grace window. The window cannot key off the file's
-- mtime: an archived replay carries an mtime from months ago and would be past
-- due the moment it is first indexed, so the window would never hold anything
-- outside a live battle. Recording first sight makes the wait real for every
-- replay regardless of how old the file is.
CREATE TABLE raw_upload_first_seen (
  replay_path TEXT PRIMARY KEY,
  first_seen  INTEGER NOT NULL
);
