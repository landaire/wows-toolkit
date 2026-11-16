-- The players met across battles, as rows rather than as one JSON blob in the
-- settings table. The blob was rewritten whole on every save, which at a few
-- hundred thousand accounts is tens of megabytes per write.
--
-- The arena and timestamp keys are separate tables because the counts they
-- drive dedup differently: the all-time count is over distinct arenas and the
-- in-range count over distinct timestamps, and two battles inside the same
-- second are two arenas under one timestamp. Pairing them would have to invent
-- which arena a shared timestamp belonged to.
CREATE TABLE tracked_player (
  account_id INTEGER PRIMARY KEY,
  last_name  TEXT NOT NULL,
  clan_id    INTEGER NOT NULL,
  clan       TEXT NOT NULL,
  notes      TEXT NOT NULL
);

-- The three tables below are key and no payload, so `WITHOUT ROWID` makes the
-- primary key their storage rather than a second index beside one: at a million
-- rows that halves what an import writes.

-- Names this account was seen under before `tracked_player.last_name`.
CREATE TABLE tracked_player_name (
  account_id INTEGER NOT NULL REFERENCES tracked_player(account_id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  PRIMARY KEY (account_id, name)
) WITHOUT ROWID;

-- One row per battle this account was met in. `in_division` marks the battles
-- shared with the recording player, which the tables can hide.
CREATE TABLE tracked_player_arena (
  account_id  INTEGER NOT NULL REFERENCES tracked_player(account_id) ON DELETE CASCADE,
  arena_id    INTEGER NOT NULL,
  in_division INTEGER NOT NULL,
  PRIMARY KEY (account_id, arena_id)
) WITHOUT ROWID;

-- The same encounters keyed by when they happened, in unix seconds.
CREATE TABLE tracked_player_timestamp (
  account_id  INTEGER NOT NULL REFERENCES tracked_player(account_id) ON DELETE CASCADE,
  seen_at     INTEGER NOT NULL,
  in_division INTEGER NOT NULL,
  PRIMARY KEY (account_id, seen_at)
) WITHOUT ROWID;
