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

-- Backfill: agents connected before this migration get a channel row with access=everyone
-- so they are not suddenly locked out after the deploy.
INSERT INTO "LiteLLM_ManagedAgentChannelsTable"
  (id, agent_id, kind, status, config, created_at, updated_at)
SELECT
  'chan_' || gen_random_uuid()::TEXT,
  id,
  'slack',
  'enabled',
  jsonb_build_object(
    'access', 'everyone',
    'allowed_user_ids', '[]'::jsonb,
    'team_id', config->'slack'->>'team_id'
  ),
  EXTRACT(EPOCH FROM NOW())::BIGINT * 1000,
  EXTRACT(EPOCH FROM NOW())::BIGINT * 1000
FROM "LiteLLM_ManagedAgentsTable"
WHERE config->'slack'->>'status' = 'connected'
  AND id NOT IN (
    SELECT agent_id FROM "LiteLLM_ManagedAgentChannelsTable" WHERE kind = 'slack'
  );
