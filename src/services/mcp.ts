import { call } from "./ipc";
import type {
  McpDesktopConflictStatus,
  McpImportSummary,
  McpOauthStatus,
  McpServer,
  McpServerInput,
  McpTarget,
  RegistryMcpServer,
} from "@/types/backend";

export async function buildMcpDeeplink(serverId: string): Promise<string> {
  return call<string>("build_mcp_deeplink", { serverId });
}

export async function listMcpServers(): Promise<McpServer[]> {
  return call<McpServer[]>("list_mcp_servers", {});
}

export async function saveMcpServer(input: McpServerInput): Promise<McpServer> {
  return call<McpServer>("save_mcp_server", { input });
}

export async function deleteMcpServer(id: string): Promise<void> {
  return call<void>("delete_mcp_server", { id });
}

export async function toggleMcpServer(
  id: string,
  target: McpTarget,
  enabled: boolean,
): Promise<void> {
  return call<void>("toggle_mcp_server", { id, target, enabled });
}

export async function reorderMcpServers(orderedIds: string[]): Promise<void> {
  return call<void>("reorder_mcp_servers", { orderedIds });
}

export async function importMcpServers(): Promise<McpImportSummary> {
  return call<McpImportSummary>("import_mcp_servers", {});
}

export async function searchMcpRegistry(query: string): Promise<RegistryMcpServer[]> {
  return call<RegistryMcpServer[]>("search_mcp_registry", { query });
}

export async function installMcpRegistryServer(
  name: string,
  enabledClaudeCode: boolean,
  enabledClaudeDesktop: boolean,
): Promise<McpServer> {
  return call<McpServer>("install_mcp_registry_server", {
    name,
    enabledClaudeCode,
    enabledClaudeDesktop,
  });
}

export async function getMcpOauthStatus(): Promise<McpOauthStatus> {
  return call<McpOauthStatus>("get_mcp_oauth_status", {});
}

export async function clearMcpOauth(serverNames: string[] = []): Promise<McpOauthStatus> {
  return call<McpOauthStatus>("clear_mcp_oauth", { input: { serverNames } });
}

export async function getMcpDesktopConflictStatus(): Promise<McpDesktopConflictStatus> {
  return call<McpDesktopConflictStatus>("get_mcp_desktop_conflict_status", {});
}

// ---- Prompt presets ---------------------------------------------------------
