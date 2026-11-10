CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
-- Keys: node_id = <ULID>, safe_mode = '0' | '1'

-- Control plane: the root directory, the authority over root names, which are unique in the
-- Space; a child's creation record is in its parent's Log.
CREATE TABLE root_directory (
  id               TEXT PRIMARY KEY,
  name             TEXT NOT NULL,
  created_at       TEXT NOT NULL,
  node             TEXT NOT NULL,
  profile          TEXT NOT NULL,
  working_dir_node TEXT NOT NULL,
  working_dir_path TEXT NOT NULL
) STRICT;

-- Control plane: archives. A Session is archived when it or an ancestor is recorded here.
CREATE TABLE archives (
  session_id TEXT PRIMARY KEY,
  at         TEXT NOT NULL
) STRICT;

-- Materialized: each row folds the Session's creation record, its archive and its own Log, and is
-- written only in the transactions that record those facts.
CREATE TABLE sessions (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  created_at    TEXT NOT NULL,
  binding_node  TEXT NOT NULL,
  binding_epoch INTEGER NOT NULL,
  profile       TEXT NOT NULL,           -- Selected profile name
  working_dir_node TEXT NOT NULL,        -- Node owning the working directory
  working_dir_path TEXT NOT NULL,        -- Absolute UTF-8 working directory
  parent        TEXT REFERENCES sessions(id),
  archived_at   TEXT
) STRICT;

CREATE UNIQUE INDEX sessions_active_name ON sessions(name) WHERE archived_at IS NULL;

CREATE TABLE log (
  session_id TEXT NOT NULL REFERENCES sessions(id),
  epoch      INTEGER NOT NULL,
  seq        INTEGER NOT NULL,
  at         TEXT NOT NULL,
  body       TEXT NOT NULL,              -- Serialized EntryBody
  kind       TEXT GENERATED ALWAYS AS (json_extract(body, '$.kind')) VIRTUAL,
  event_id   TEXT GENERATED ALWAYS AS (CASE WHEN json_extract(body, '$.kind') = 'event_consumed' THEN json_extract(body, '$.event.id') END) VIRTUAL,
  plan       TEXT GENERATED ALWAYS AS (CASE WHEN json_extract(body, '$.kind') = 'attempt_started' THEN json_extract(body, '$.plan') END) VIRTUAL,
  PRIMARY KEY (session_id, epoch, seq),
  FOREIGN KEY (plan) REFERENCES context_plans(hash)
) STRICT;

CREATE UNIQUE INDEX log_event_id ON log(event_id) WHERE event_id IS NOT NULL;

CREATE TABLE tool_catalogs (
  hash TEXT PRIMARY KEY,
  body BLOB NOT NULL                     -- zstd-compressed frozen tool definitions
) STRICT;

CREATE TABLE context_plans (
  hash    TEXT PRIMARY KEY,              -- Complete ContextPlan identity, independent of encoding
  catalog TEXT NOT NULL REFERENCES tool_catalogs(hash),
  parent  TEXT REFERENCES context_plans(hash),
  body    BLOB NOT NULL                  -- zstd-compressed items, output budget and omissions
) STRICT;

CREATE TABLE evidence (
  hash TEXT PRIMARY KEY,                 -- Address of the uncompressed bytes
  body BLOB NOT NULL                     -- zstd-compressed mapping record or response it read
) STRICT;

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

CREATE TABLE connection_updates (
  connection TEXT NOT NULL REFERENCES connections(key),
  update_key TEXT NOT NULL,
  PRIMARY KEY (connection, update_key)
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

-- Control plane: what was registered, the same on every node. Identities derive from
-- registrations, so no table here counts anything.

-- A registration groups accepted content; each node's references may recover independently.
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

-- A name a registration wrote for a plugin, or its removal; the first one names the identity.
CREATE TABLE registration_names (
  registration INTEGER NOT NULL REFERENCES registrations(id),
  plugin_id    TEXT NOT NULL,
  name         TEXT,
  PRIMARY KEY (registration, plugin_id)
) STRICT;

-- Content a registration brought for a plugin, identified by the two, and where its code came
-- from: the owner's code intent (a source directory, or factory code with settings), or the code
-- new settings reuse (an earlier generation of the plugin, or factory code by its artifact).
CREATE TABLE plugin_generations (
  registration        INTEGER NOT NULL REFERENCES registrations(id),
  plugin_id           TEXT NOT NULL,
  artifact            TEXT NOT NULL,
  settings            TEXT NOT NULL,
  intent              TEXT CHECK (intent IN ('directory', 'factory')),
  source              TEXT,
  based_on_generation INTEGER,
  based_on_factory    TEXT,
  PRIMARY KEY (registration, plugin_id),
  FOREIGN KEY (based_on_generation, plugin_id) REFERENCES plugin_generations(registration, plugin_id),
  CHECK ((intent IS 'directory') = (source IS NOT NULL)),
  CHECK ((intent IS NULL) = (based_on_generation IS NOT NULL OR based_on_factory IS NOT NULL)),
  CHECK (based_on_generation IS NULL OR based_on_factory IS NULL)
) STRICT;

-- The code a registration activated for a plugin: the generation it brought, or factory code it
-- chose to follow, by artifact.
CREATE TABLE registration_code (
  registration INTEGER NOT NULL REFERENCES registrations(id),
  plugin_id    TEXT NOT NULL,
  factory      TEXT,
  generation   INTEGER GENERATED ALWAYS AS (CASE WHEN factory IS NULL THEN registration END) VIRTUAL,
  PRIMARY KEY (registration, plugin_id),
  FOREIGN KEY (generation, plugin_id) REFERENCES plugin_generations(registration, plugin_id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

-- A registered configuration document, identified by its registration.
CREATE TABLE config_generations (
  registration INTEGER PRIMARY KEY REFERENCES registrations(id),
  document     TEXT NOT NULL,
  source       TEXT NOT NULL
) STRICT;

-- Node: what runs here and how it fared; another node keeps its own.

-- Each plugin's current and rollback references on this node. `_by` is the registration that
-- wrote a reference and activated its code; `_factory` is factory code's artifact. A reference
-- without an artifact is the generation its registration brought; factory code without a
-- registration is what recovery reached.
CREATE TABLE plugins (
  id                  TEXT PRIMARY KEY,
  named_by            INTEGER,
  active_by           INTEGER,
  active_factory      TEXT,
  active_generation   INTEGER GENERATED ALWAYS AS (CASE WHEN active_factory IS NULL THEN active_by END) VIRTUAL,
  rollback_by         INTEGER,
  rollback_factory    TEXT,
  rollback_generation INTEGER GENERATED ALWAYS AS (CASE WHEN rollback_factory IS NULL THEN rollback_by END) VIRTUAL,
  FOREIGN KEY (named_by, id) REFERENCES registration_names(registration, plugin_id),
  FOREIGN KEY (active_by, id) REFERENCES registration_code(registration, plugin_id),
  FOREIGN KEY (rollback_by, id) REFERENCES registration_code(registration, plugin_id),
  FOREIGN KEY (active_generation, id) REFERENCES plugin_generations(registration, plugin_id),
  FOREIGN KEY (rollback_generation, id) REFERENCES plugin_generations(registration, plugin_id)
) STRICT;

-- Each generation's trial, health or failure on this node.
CREATE TABLE generation_health (
  registration INTEGER NOT NULL,
  plugin_id    TEXT NOT NULL,
  status       TEXT NOT NULL CHECK (status IN ('trial', 'healthy', 'failed')),
  failure      TEXT,
  PRIMARY KEY (registration, plugin_id),
  FOREIGN KEY (registration, plugin_id) REFERENCES plugin_generations(registration, plugin_id),
  CHECK ((status = 'failed') = (failure IS NOT NULL))
) STRICT;

-- The configuration that runs on this node.
CREATE TABLE config (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  active    INTEGER NOT NULL REFERENCES config_generations(registration)
) STRICT;

-- The name each registration replaced on this node, read by the owner's rollback here; absent
-- `previous` means the plugin had no name.
CREATE TABLE replaced_names (
  registration INTEGER NOT NULL,
  plugin_id    TEXT NOT NULL,
  previous     INTEGER,
  PRIMARY KEY (registration, plugin_id),
  FOREIGN KEY (registration, plugin_id) REFERENCES registration_names(registration, plugin_id),
  FOREIGN KEY (previous, plugin_id) REFERENCES registration_names(registration, plugin_id)
) STRICT;

-- The configuration a registration replaced on this node when it activated its own.
CREATE TABLE replaced_config (
  registration INTEGER PRIMARY KEY REFERENCES config_generations(registration),
  previous     INTEGER NOT NULL REFERENCES config_generations(registration)
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
