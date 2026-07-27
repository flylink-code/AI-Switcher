/**
 * Shared types for values exchanged with the Rust backend over Tauri IPC.
 * Keep these in sync with the corresponding serde structs in src-tauri/src/.
 */

export type ProtocolType = "anthropic" | "proxy" | "openai_chat" | "openai_responses";
export type ProviderTarget = "claude_code" | "claude_desktop";

export interface ClaudeModelMapping {
  sonnet: string;
  opus: string;
  haiku: string;
  fable: string;
  subagent: string;
}

/** A single API provider (mirrors `crate::provider::Provider`). */
export interface Provider {
  id: string;
  name: string;
  baseUrl: string;
  /** API keys are never returned over IPC. */
  apiKeySet: boolean;
  model: string;
  modelMapping: ClaudeModelMapping;
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
  /** Canonical HTTPS origin or gateway path prefix; never a complete request endpoint. */
  baseUrl: string;
  apiKey: string;
  clearApiKey?: boolean;
  model: string;
  modelMapping: ClaudeModelMapping;
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

export interface ProviderHealthUpdated {
  providerId: string;
  targetApp: ProviderTarget;
  ok: boolean;
  category: string;
  message: string;
  checkedAt: number;
}

export interface ModelDiscoveryResult {
  models: string[];
  message: string;
  checkedAt: number;
  source: "network" | "cache" | "none";
  stale: boolean;
  expiresAt?: number | null;
  error?: string | null;
}

export interface ProviderImportResult {
  imported: number;
  skipped: number;
}

export interface ConfigBackup {
  name: string;
  createdAt: number;
  verified: boolean;
  sourceName?: string | null;
}

export interface DesktopLocalizationStatus {
  platformSupported: boolean;
  installDetected: boolean;
  installKind?: string | null;
  detectionSource: string;
  checkedAt: number;
  diagnostics: string[];
  installPath?: string | null;
  resourcesPath?: string | null;
  claudeVersion?: string | null;
  multipleInstalls: boolean;
  state: "unsupported" | "notInstalled" | "installed" | "partial";
  configuredLocale?: string | null;
  packPath?: string | null;
  packValid: boolean;
  packSource?: "local" | "github" | null;
  packVersion?: string | null;
  packRevision?: string | null;
  packFetchedAt?: number | null;
  backupAvailable: boolean;
  message: string;
}

export interface DesktopLocalizationPackInfo {
  source: "local" | "github";
  version?: string | null;
  revision?: string | null;
  fetchedAt?: number | null;
  packPath: string;
  valid: boolean;
}

export interface DesktopLocalizationPackValidation {
  valid: boolean;
  packPath: string;
  message: string;
}

export interface DesktopLocalizationActionResult {
  ok: boolean;
  changedFiles: number;
  message: string;
  logPath?: string | null;
}

export interface ClaudeCodeLocalizationStatus {
  installed: boolean;
  version?: string | null;
  executablePath?: string | null;
  pluginEnabled: boolean;
  settingsConfigured: boolean;
  message: string;
}

export interface EditorLocalizationStatus {
  id: "vscode" | "cursor";
  label: string;
  editorDetected: boolean;
  editorCliPath?: string | null;
  claudeExtensionPath?: string | null;
  helperInstalled: boolean;
  message: string;
}

export interface LocalizationHubStatus {
  claudeCode: ClaudeCodeLocalizationStatus;
  editors: EditorLocalizationStatus[];
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
  /** Detected Claude Desktop 1p install dir. */
  claudeDesktopBase: string | null;
  /** Detected Claude Desktop 3p install dir. */
  claudeDesktopThreepBase: string | null;
  /** Claude Desktop `configLibrary` dir (under 3p), or null. */
  claudeDesktopConfigLibrary: string | null;
  /** `configLibrary/_meta.json`, or null. */
  claudeDesktopMetaPath: string | null;
  /** `Claude/claude_desktop_config.json`, or null. */
  claudeDesktopNormalConfigPath: string | null;
  /** `Claude-3p/claude_desktop_config.json`, or null. */
  claudeDesktopThreepConfigPath: string | null;
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
  phase: "stopped" | "starting" | "running" | "error";
  lastError: string | null;
  checkedAt: number;
}

export interface ProxyStatusUpdated {
  target: ProviderTarget;
  status: ProxyStatus;
}

export type AutostartMode = "off" | "silent" | "window";
export type CloseBehavior = "ask" | "tray" | "quit";

export interface AutostartConfig {
  enabled: boolean;
  mode: AutostartMode;
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

export interface RegistryMcpServer {
  name: string;
  title: string;
  description: string;
  version: string;
  installable: boolean;
  supportNote: string;
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

export interface RepositorySkill {
  name: string;
  path: string;
  description: string;
}

export interface UsageSummary {
  requestCount: number;
  successfulRequestCount: number;
  inputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
  outputTokens: number;
  estimatedCost: number;
}

export interface UsageBreakdown {
  key: string;
  requestCount: number;
  inputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
  outputTokens: number;
  estimatedCost: number;
}

export interface UsageTrendPoint {
  date: string;
  requestCount: number;
  inputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
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
  deletedByAge: number;
  deletedByLimit: number;
  integrityOk: boolean;
}

export interface LogMaintenancePolicy {
  retentionDays: number;
  maxRows: number;
  autoMaintain: boolean;
}

export interface LogMaintenancePreview {
  totalRows: number;
  deleteByAge: number;
  deleteByLimit: number;
}

export interface ProxyRequestLog {
  id: string;
  createdAt: number;
  providerId: string | null;
  providerName: string | null;
  model: string | null;
  statusCode: number | null;
  inputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
  outputTokens: number;
  durationMs: number;
  targetApp: string | null;
  protocol: string | null;
  route: string | null;
  isStream: boolean;
  errorCategory: string | null;
  diagnostic: string | null;
}

export interface PaginatedProxyLogs {
  data: ProxyRequestLog[];
  total: number;
  page: number;
  pageSize: number;
}

export interface ProxyLogListInput {
  days?: number;
  targetApp?: string;
  statusCode?: number;
  page?: number;
  pageSize?: number;
}

export interface ClaudeCodeVersionInfo {
  installed: boolean;
  currentVersion: string | null;
  latestVersion: string | null;
  updateAvailable: boolean;
  installCommand: string;
  updateCommand: string;
  error: string | null;
  executablePath: string | null;
  source: string | null;
  environment: string;
  installedButBroken: boolean;
  wslDistro: string | null;
}

export type SessionProvider = "claude_code" | "claude_desktop";

export interface SessionProviderStatus {
  provider: SessionProvider;
  status: "available" | "not_found" | "unsupported_format";
  detail: string;
  rootPath?: string;
}

export interface SessionMeta {
  provider: SessionProvider;
  sessionId: string;
  title?: string;
  summary?: string;
  projectDir?: string;
  createdAt?: number;
  lastActiveAt?: number;
  sourcePath: string;
  resumeCommand?: string;
}

export interface SessionMessage {
  role: string;
  content: string;
  timestamp?: number;
}

export interface SessionScanResult {
  sessions: SessionMeta[];
  providers: SessionProviderStatus[];
}
