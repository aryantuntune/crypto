CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('user','assistant','system')),
  content TEXT NOT NULL,
  image_path TEXT,
  prediction_id INTEGER REFERENCES predictions(id)
);
CREATE INDEX IF NOT EXISTS idx_messages_ts ON messages(ts);

CREATE TABLE IF NOT EXISTS predictions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  symbol TEXT NOT NULL,
  horizon_seconds INTEGER NOT NULL,
  predicted_prob_up REAL NOT NULL,
  recommended_action TEXT NOT NULL,
  stop_loss_pct REAL,
  take_profit_pct REAL,
  citations_json TEXT,
  outcome_status TEXT NOT NULL DEFAULT 'pending',
  outcome_price_at_ts REAL,
  outcome_price_at_horizon REAL,
  outcome_resolved_at INTEGER
);

CREATE TABLE IF NOT EXISTS cost_daily (
  date TEXT PRIMARY KEY,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  cached_input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_usd REAL NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
