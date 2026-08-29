import { invoke, isTauri } from "@tauri-apps/api/core";

export type McpTransportConfig =
  | { type: "streamableHttp"; url: string; hasBearerToken: boolean }
  | { type: "stdio"; command: string; args: string[]; cwd: string | null; env: Record<string, string> };

export type McpServerConfig = {
  id: string;
  name: string;
  enabled: boolean;
  transport: McpTransportConfig;
  allowedTools: string[];
  autoApproveTools: string[];
  startupTimeoutMs: number;
  callTimeoutMs: number;
  maxOutputChars: number;
  maxConcurrency: number;
  pluginId: string | null;
};

export type McpConnectionState = "disabled" | "cached" | "connecting" | "ready" | "backoff" | "failed";

export type McpToolSnapshot = {
  serverId: string;
  serverName: string;
  remoteName: string;
  wireName: string;
  description: string;
  inputSchema: unknown;
  readOnlyHint: boolean;
  destructiveHint: boolean;
  idempotentHint: boolean;
  openWorldHint: boolean;
  autoApproved: boolean;
  maxOutputChars: number;
  catalogRevision: string;
  pluginId: string | null;
};

export type McpServerView = McpServerConfig & {
  status: {
    serverId: string;
    state: McpConnectionState;
    toolCount: number;
    catalogRevision: string | null;
    lastSuccessAt: number | null;
    lastError: string | null;
    consecutiveFailures: number;
    retryAfter: number | null;
  };
  tools: McpToolSnapshot[];
};

export type McpOverview = { servers: McpServerView[] };

export function listMcpServers(): Promise<McpOverview> {
  if (!isTauri()) return Promise.resolve({ servers: [] });
  return invoke<McpOverview>("mcp_list_servers");
}

export function upsertMcpServer(config: McpServerConfig, bearerToken?: string): Promise<McpServerView> {
  return invoke<McpServerView>("mcp_upsert_server", { config, bearerToken });
}

export function setMcpServerEnabled(serverId: string, enabled: boolean): Promise<McpServerView> {
  return invoke<McpServerView>("mcp_set_server_enabled", { serverId, enabled });
}

export function refreshMcpServer(serverId: string): Promise<McpServerView> {
  return invoke<McpServerView>("mcp_refresh_server", { serverId });
}

export function removeMcpServer(serverId: string): Promise<boolean> {
  return invoke<boolean>("mcp_remove_server", { serverId });
}
