CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
-- Keys: node_id = <ULID>, safe_mode = '0' | '1'

CREATE TABLE sessions (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL UNIQUE,
  created_at    TEXT NOT NULL,
  binding_node  TEXT NOT NULL,
  binding_epoch INTEGER NOT NULL,
  profile       TEXT NOT NULL            -- Selected profile name
) STRICT;

CREATE TABLE log (
  session_id TEXT NOT NULL REFERENCES sessions(id),
  epoch      INTEGER NOT NULL,
  seq        INTEGER NOT NULL,
  at         TEXT NOT NULL,
  body       TEXT NOT NULL,              -- Serialized EntryBody
  kind       TEXT GENERATED ALWAYS AS (json_extract(body, '$.kind')) VIRTUAL,
  PRIMARY KEY (session_id, epoch, seq)
) STRICT;

CREATE TABLE inbox (
  order_no       INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id       TEXT NOT NULL UNIQUE,
  session_id     TEXT NOT NULL REFERENCES sessions(id),
  event          TEXT NOT NULL,          -- Serialized Event
  consumed_epoch INTEGER,
  consumed_seq   INTEGER
) STRICT;
CREATE INDEX inbox_pending ON inbox(session_id, order_no) WHERE consumed_seq IS NULL;

CREATE TABLE schedules (
  id             TEXT PRIMARY KEY,
  session_id     TEXT NOT NULL REFERENCES sessions(id),
  rule           TEXT NOT NULL,
  message        TEXT NOT NULL,
  created_at     TEXT NOT NULL,
  state          TEXT NOT NULL CHECK (state IN ('active', 'done', 'cancelled')),
  last_due       TEXT,
  last_event_id  TEXT,
  CHECK ((last_due IS NULL) = (last_event_id IS NULL))
) STRICT;

CREATE TABLE connections (
  key   TEXT PRIMARY KEY,
  state TEXT NOT NULL
) STRICT;

-- One final outcome per logical delivery; the body contains no reply text.
CREATE TABLE deliveries (
  order_no   INTEGER PRIMARY KEY AUTOINCREMENT,
  connection TEXT NOT NULL REFERENCES connections(key),
  session_id TEXT NOT NULL,
  epoch      INTEGER NOT NULL,
  seq        INTEGER NOT NULL,
  body       TEXT NOT NULL,
  outcome    TEXT GENERATED ALWAYS AS (json_extract(body, '$.outcome.kind')) VIRTUAL,
  UNIQUE (connection, session_id, epoch, seq),
  FOREIGN KEY (session_id, epoch, seq) REFERENCES log(session_id, epoch, seq)
) STRICT;
CREATE INDEX delivery_failures ON deliveries(connection, order_no)
  WHERE outcome IN ('failed', 'unknown');

-- Registry state is committed by the single Registry writer.
CREATE TABLE plugins (
  id     TEXT PRIMARY KEY,
  active INTEGER REFERENCES generations(id)
) STRICT;

CREATE TABLE generations (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  plugin_id  TEXT NOT NULL REFERENCES plugins(id),
  artifact   TEXT NOT NULL,
  config     TEXT NOT NULL,
  origin     TEXT NOT NULL CHECK (origin IN ('factory', 'deployed')),
  status     TEXT NOT NULL CHECK (status IN ('trial', 'healthy', 'failed')),
  failure    TEXT,                       -- Serialized Failure; present only when failed
  created_at TEXT NOT NULL,
  CHECK ((status = 'failed') = (failure IS NOT NULL))
) STRICT;
CREATE INDEX generations_by_plugin ON generations(plugin_id, id);
