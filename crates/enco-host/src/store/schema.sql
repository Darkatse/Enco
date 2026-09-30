CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
-- Keys: schema_version = '1', node_id = <ULID>, safe_mode = '0' | '1'

CREATE TABLE sessions (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL UNIQUE,
  created_at    TEXT NOT NULL,
  binding_node  TEXT NOT NULL,
  binding_epoch INTEGER NOT NULL,
  config        TEXT NOT NULL            -- Serialized SessionConfig
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
  due_at         TEXT NOT NULL,
  message        TEXT NOT NULL,
  created_at     TEXT NOT NULL,
  state          TEXT NOT NULL CHECK (state IN ('pending', 'fired', 'cancelled')),
  fired_event_id TEXT
) STRICT;
CREATE INDEX schedules_pending ON schedules(due_at) WHERE state = 'pending';

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
