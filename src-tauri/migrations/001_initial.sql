CREATE TABLE devices (
  device_id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  platform TEXT NOT NULL,
  identity_public_key BLOB NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE trusted_peers (
  peer_device_id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  public_key BLOB NOT NULL,
  first_seen_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL
);

CREATE TABLE rooms (
  room_id TEXT PRIMARY KEY,
  host_device_id TEXT NOT NULL,
  media_type TEXT NOT NULL,
  state TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  ended_at INTEGER
);

CREATE TABLE schedules (
  schedule_id TEXT PRIMARY KEY,
  room_id TEXT NOT NULL,
  media_id TEXT NOT NULL,
  scheduled_start INTEGER NOT NULL,
  planned_preload_start INTEGER NOT NULL,
  guest_device_id TEXT NOT NULL,
  status TEXT NOT NULL
);

CREATE TABLE media_items (
  media_id TEXT PRIMARY KEY,
  media_type TEXT NOT NULL,
  title TEXT NOT NULL,
  source_uri TEXT,
  filename TEXT,
  file_size INTEGER,
  duration_ms INTEGER NOT NULL,
  full_hash TEXT
);

CREATE TABLE cache_entries (
  media_id TEXT PRIMARY KEY,
  cache_path TEXT NOT NULL,
  bytes_available INTEGER NOT NULL,
  complete BOOLEAN NOT NULL,
  keep_policy TEXT NOT NULL,
  last_accessed INTEGER NOT NULL
);

CREATE TABLE providers (
  provider_id TEXT PRIMARY KEY,
  adapter_version TEXT NOT NULL,
  profile_path TEXT NOT NULL,
  last_verified INTEGER
);

CREATE TABLE network_history (
  id INTEGER PRIMARY KEY,
  peer_device_id TEXT NOT NULL,
  connection_type TEXT NOT NULL,
  rtt_ms REAL NOT NULL,
  goodput_bps INTEGER NOT NULL,
  timestamp INTEGER NOT NULL
);

CREATE TABLE chat_messages (
  message_id TEXT PRIMARY KEY,
  room_id TEXT NOT NULL,
  sender_device_id TEXT NOT NULL,
  body TEXT NOT NULL,
  sent_at INTEGER NOT NULL
);
