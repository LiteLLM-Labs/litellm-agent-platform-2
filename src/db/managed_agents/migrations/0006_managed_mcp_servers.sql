CREATE TABLE IF NOT EXISTS "LiteLLM_ManagedMcpServersTable" (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  url TEXT NOT NULL,
  auth_type TEXT NOT NULL DEFAULT 'api_key',
  auth_value TEXT,
  description TEXT,
  created_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS "LiteLLM_ManagedMcpServers_name_idx"
  ON "LiteLLM_ManagedMcpServersTable" (name);

ALTER TABLE "LiteLLM_ManagedAgentsTable"
  ADD COLUMN IF NOT EXISTS mcp_server_ids JSONB NOT NULL DEFAULT '[]'::jsonb;
