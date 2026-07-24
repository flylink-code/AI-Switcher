/**
 * Shared types for values exchanged with the Rust backend over Tauri IPC.
 * Keep these in sync with the corresponding serde structs in src-tauri/src/.
 */

export type ProtocolType = "anthropic" | "proxy" | "openai_chat" | "openai_responses";
export type ProviderTarget = "claude_code" | "claude_desktop";

/** A single API provider (mirrors `crate::provider::Provider`). */
export interface Provider {
  id: string;
  name: string;
  baseUrl: string;
  /** API keys are never returned over IPC. */
  apiKeySet: boolean;
  model: string;
  protocolType: ProtocolType;
  targetApp: ProviderTarget;
  notes: string;
  sortIndex: number;
  isCurrent: boolean;
  createdAt: number;
  healthStatus?: string | null;
  healthCheckedAt?: number | null;
}

/** Input shape for create/update commands (mirrors `ProviderInput`). */
export interface ProviderInput {
  /** Omitted/undefined on create; required on update. */
  id?: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  clearApiKey?: boolean;
  model: string;
  protocolType: ProtocolType;
  targetApp: ProviderTarget;
  notes: string;
}

export interface ConnectionTestResult {
  ok: boolean;
  category: string;
  message: string;
  checkedAt: number;
}

export interface ModelDiscoveryResult {
  models: string[];
  message: string;
  checkedAt: number;
}

export interface ProviderImportResult {
  imported: number;
  skipped: number;
}

export interface ConfigBackup {
  name: string;
  createdAt: number;
}

/** Result of the `get_paths` command — surfaced on the Environment page to verify P0 detection. */
export interface PathsInfo {
  /** Resolved user home directory. */
  home: string;
  /** `~/.claude` config directory. */
  claudeConfigDir: string;
  /** `~/.claude/settings.json`. */
  claudeSettingsPath: string;
  /** `~/.claude.json` (MCP + project roots). */
  claudeJsonPath: string;
  /** Application data directory (`~/.claude-switcher`). */
  appConfigDir: string;
  /** Main SQLite database path. */
  appDbPath: string;
  /** Backup directory. */
  backupDir: string;
  /** Detected Claude Desktop base dir, or null when Claude Desktop is not installed. */
  claudeDesktopBase: string | null;
  /** Claude Desktop `configLibrary` dir, or null. */
  claudeDesktopConfigLibrary: string | null;
  /** `configLibrary/_meta.json`, or null. */
  claudeDesktopMetaPath: string | null;
}

/** Basic database info returned by `get_db_info`. */
export interface DbInfo {
  path: string;
  schemaVersion: number;
  providerCount: number;
}

/** Result of proxy status commands. */
export interface ProxyStatus {
  running: boolean;
  port: number;
  targetProvider: string | null;
}

/** One unified MCP server definition. */
export interface McpServer {
  id: string;
  name: string;
  /** Raw JSON entry stored under `mcpServers.<name>`. */
  serverConfig: Record<string, unknown>;
  enabledClaudeCode: boolean;
  enabledClaudeDesktop: boolean;
  createdAt: number;
}

/** Input shape for creating or updating an MCP server. */
export interface McpServerInput {
  id?: string;
  name: string;
  serverConfig: Record<string, unknown>;
  enabledClaudeCode: boolean;
  enabledClaudeDesktop: boolean;
}

export type McpTarget = "claude_code" | "claude_desktop";

export interface McpImportSummary {
  imported: number;
  updated: number;
}

/** A stored CLAUDE.md prompt preset. */
export interface PromptInfo {
  name: string;
  updatedAt: number;
}

export interface PromptDetail extends PromptInfo {
  content: string;
}

export interface LivePrompt {
  path: string;
  content: string;
  updatedAt: number;
}

export interface Skill {
  name: string;
  path: string;
  enabled: boolean;
  description: string;
}

export interface UsageSummary {
  requestCount: number;
  successfulRequestCount: number;
  inputTokens: number;
  outputTokens: number;
  estimatedCost: number;
}

export interface UsageBreakdown {
  key: string;
  requestCount: number;
  inputTokens: number;
  outputTokens: number;
  estimatedCost: number;
}

export interface UsageTrendPoint {
  date: string;
  requestCount: number;
  inputTokens: number;
  outputTokens: number;
  estimatedCost: number;
}

export interface UsageDashboard {
  summary: UsageSummary;
  byProvider: UsageBreakdown[];
  byModel: UsageBreakdown[];
  trend: UsageTrendPoint[];
}

export interface ModelPricing {
  model: string;
  inputPricePerMillion: number;
  outputPricePerMillion: number;
  currency: string;
}

export interface ModelPricingInput {
  model: string;
  inputPricePerMillion: number;
  outputPricePerMillion: number;
  currency: string;
}

export interface LogMaintenanceResult {
  deleted: number;
  integrityOk: boolean;
}
