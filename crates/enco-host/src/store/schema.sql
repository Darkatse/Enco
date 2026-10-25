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
  event_id   TEXT GENERATED ALWAYS AS (CASE WHEN json_extract(body, '$.kind') = 'event_consumed' THEN json_extract(body, '$.event.id') END) VIRTUAL,
  PRIMARY KEY (session_id, epoch, seq)
) STRICT;

CREATE UNIQUE INDEX log_event_id ON log(event_id) WHERE event_id IS NOT NULL;

CREATE TABLE inbox (
  order_no       INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id       TEXT NOT NULL UNIQUE,
  session_id     TEXT NOT NULL REFERENCES sessions(id),
  event          TEXT NOT NULL           -- Serialized Event
) STRICT;
CREATE INDEX inbox_pending ON inbox(session_id, order_no);

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

-- A registration groups accepted content; runtime references may recover independently.
CREATE TABLE registrations (
  id         INTEGER PRIMARY KEY,
  initiator  TEXT NOT NULL CHECK (initiator IN ('owner', 'host', 'agent')),
  session    TEXT REFERENCES sessions(id),
  call       TEXT,
  reason     TEXT,
  created_at TEXT NOT NULL,
  CHECK ((initiator = 'agent') = (session IS NOT NULL)),
  CHECK ((initiator = 'agent') = (call IS NOT NULL)),
  CHECK (initiator <> 'host' OR reason IS NULL),
  CHECK (initiator <> 'agent' OR reason IS NOT NULL)
) STRICT;

CREATE TABLE generations (
  id           INTEGER PRIMARY KEY,
  created_at   TEXT NOT NULL
) STRICT;

CREATE TABLE plugins (
  id              TEXT PRIMARY KEY,
  named_by        INTEGER,
  active          INTEGER,
  active_by       INTEGER,
  rollback_target INTEGER,
  rollback_by     INTEGER,
  FOREIGN KEY (named_by, id) REFERENCES registration_names(registration, plugin_id),
  FOREIGN KEY (active, id) REFERENCES plugin_generations(id, plugin_id),
  FOREIGN KEY (rollback_target, id) REFERENCES plugin_generations(id, plugin_id),
  FOREIGN KEY (active_by, id) REFERENCES registration_code(registration, plugin_id),
  FOREIGN KEY (rollback_by, id) REFERENCES registration_code(registration, plugin_id),
  CHECK (active IS NOT NULL OR active_by IS NULL),
  CHECK (rollback_target IS NOT NULL OR rollback_by IS NULL)
) STRICT;

CREATE TABLE registration_names (
  registration INTEGER NOT NULL REFERENCES registrations(id),
  plugin_id    TEXT NOT NULL REFERENCES plugins(id),
  name         TEXT,
  previous     INTEGER,
  PRIMARY KEY (registration, plugin_id),
  FOREIGN KEY (previous, plugin_id) REFERENCES registration_names(registration, plugin_id)
) STRICT;

CREATE TABLE registration_code (
  registration INTEGER NOT NULL REFERENCES registrations(id),
  plugin_id    TEXT NOT NULL REFERENCES plugins(id),
  generation   INTEGER NOT NULL,
  intent       TEXT CHECK (intent IN ('directory', 'factory')),
  source       TEXT,
  CHECK ((intent IS 'directory') = (source IS NOT NULL)),
  PRIMARY KEY (registration, plugin_id),
  FOREIGN KEY (generation, plugin_id) REFERENCES plugin_generations(id, plugin_id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE plugin_generations (
  id         INTEGER PRIMARY KEY REFERENCES generations(id),
  plugin_id  TEXT NOT NULL REFERENCES plugins(id),
  artifact   TEXT NOT NULL,
  settings   TEXT NOT NULL,
  registration INTEGER REFERENCES registrations(id),
  origin     TEXT NOT NULL CHECK (origin IN ('factory', 'code', 'settings', 'artifact')),
  based_on   INTEGER REFERENCES plugin_generations(id),
  status     TEXT NOT NULL CHECK (status IN ('trial', 'healthy', 'failed')),
  failure    TEXT,
  UNIQUE (id, plugin_id),
  CHECK ((status = 'failed') = (failure IS NOT NULL)),
  CHECK ((origin = 'factory') = (registration IS NULL)),
  CHECK ((origin = 'settings') = (based_on IS NOT NULL))
) STRICT;
CREATE INDEX plugin_generations_by_plugin ON plugin_generations(plugin_id, id);

CREATE TABLE config_generations (
  id       INTEGER PRIMARY KEY REFERENCES generations(id),
  registration INTEGER NOT NULL REFERENCES registrations(id),
  document TEXT NOT NULL,
  source   TEXT NOT NULL,
  previous INTEGER REFERENCES config_generations(id)
) STRICT;

CREATE TABLE config (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  active    INTEGER NOT NULL REFERENCES config_generations(id)
) STRICT;

-- Reuse is a build policy, not a promise of bit-identical recompilation.
CREATE TABLE builds (
  source     TEXT NOT NULL,
  sdk        TEXT NOT NULL,
  artifact   TEXT NOT NULL,
  toolchain  TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (source, sdk)
) STRICT;

-- The navigation manual of a plugin source, rendered from rustdoc when the source was built.
CREATE TABLE manuals (
  source TEXT PRIMARY KEY,
  manual TEXT NOT NULL
) STRICT;
