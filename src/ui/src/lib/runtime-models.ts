import type { AgentRuntimeId, ModelOption } from "@/lib/types";

export function providerForRuntime(runtime?: string): string | null {
  if (runtime === "claude_managed_agents" || runtime === "claude_agents") return "anthropic";
  if (runtime === "cursor") return "cursor";
  if (runtime === "opencode") return "opencode";
  return null;
}

export function filterModelsForRuntime(models: ModelOption[], runtime?: string): ModelOption[] {
  const provider = providerForRuntime(runtime);
  if (!provider) return models;
  return models.filter((model) => modelMatchesProvider(model, provider));
}

export function modelIdsForRuntime(models: ModelOption[], runtime?: string, _current?: string): string[] {
  return filterModelsForRuntime(models, runtime).map((model) => model.id);
}

export function defaultModelForRuntime(
  models: ModelOption[],
  runtime: AgentRuntimeId | string,
  current?: string,
): string {
  const ids = modelIdsForRuntime(models, runtime);
  if (current && ids.includes(current)) return current;
  return ids[0] ?? current ?? "";
}

function modelMatchesProvider(model: ModelOption, provider: string): boolean {
  const id = model.id.toLowerCase();
  const upstream = (model.upstream_model ?? "").toLowerCase();
  const configuredProvider = (model.provider ?? model.owned_by ?? "").toLowerCase();
  if (configuredProvider === provider) return true;
  if (id.startsWith(`${provider}/`) || upstream.startsWith(`${provider}/`)) return true;
  if (provider === "anthropic") return id.includes("claude") || upstream.includes("claude");
  return false;
}
