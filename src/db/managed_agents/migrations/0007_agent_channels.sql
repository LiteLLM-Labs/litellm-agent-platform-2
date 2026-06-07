CREATE TABLE IF NOT EXISTS "LiteLLM_ManagedAgentChannelsTable" (
  id         TEXT PRIMARY KEY,
  agent_id   TEXT NOT NULL REFERENCES "LiteLLM_ManagedAgentsTable"(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,
  status     TEXT NOT NULL DEFAULT 'enabled',
  config     JSONB NOT NULL DEFAULT '{}',
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL,
  UNIQUE (agent_id, kind)
);

CREATE INDEX IF NOT EXISTS "LiteLLM_ManagedAgentChannels_agent_id_idx"
  ON "LiteLLM_ManagedAgentChannelsTable" (agent_id);
