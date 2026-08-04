/**
 * Shared types for values exchanged with the Rust backend over Tauri IPC.
 * Keep these in sync with the corresponding serde structs in src-tauri/src/.
 */

export type ProtocolType = "anthropic" | "proxy" | "openai_chat" | "openai_responses";
export type ProviderTarget = "claude_code" | "claude_desktop" | "codex";
export type ProviderKind = "standard" | "codex_oauth";

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
  /** Optional Codex catalog context window; missing uses 272k. */
  modelContextWindow?: number | null;
  /** Optional Codex auto-review subagent model override. */
  autoReviewModelOverride?: string | null;
  /** Optional Codex web-search capability override; missing means automatic. */
  webSearchEnabled?: boolean | null;
  modelMapping: ClaudeModelMapping;
  protocolType: ProtocolType;
  providerKind: ProviderKind;
  authBinding: string;
  targetApp: ProviderTarget;
  notes: string;
  sortIndex: number;
  /** Lower = higher failover priority. */
  failoverGroup: number;
  /** Empty = any model; otherwise request/mapped model must match. */
  failoverModels: string[];
  isCurrent: boolean;
  createdAt: number;
  healthStatus?: string | null;
  healthCheckedAt?: number | null;
  /** Last probe latency in ms (session/UI only; may come from health event). */
  healthLatencyMs?: number | null;
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
  modelContextWindow?: number | null;
  autoReviewModelOverride?: string | null;
  webSearchEnabled?: boolean | null;
  modelMapping: ClaudeModelMapping;
  protocolType: ProtocolType;
  providerKind?: ProviderKind;
  authBinding?: string;
  targetApp: ProviderTarget;
  notes: string;
  failoverGroup?: number;
  failoverModels?: string[];
}

export interface ConnectionTestResult {
  ok: boolean;
  category: string;
  message: string;
  checkedAt: number;
  latencyMs?: number | null;
}

export interface ProviderHealthUpdated {
  providerId: string;
  targetApp: ProviderTarget;
  ok: boolean;
  category: string;
  message: string;
  checkedAt: number;
  latencyMs?: number | null;
}

export interface CodexProviderSyncResult {
  status: string;
  message: string;
  targetProvider: string;
  backupDir?: string | null;
  changedSessionFiles: number;
  sqliteRowsUpdated: number;
  skippedLockedFiles: string[];
}

export interface SwitchProviderResult {
  provider: Provider;
  sessionSync?: CodexProviderSyncResult | null;
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

export type ImportResource = "provider" | "mcp";

export interface ImportPreviewItem {
  name: string;
  summary: string;
  detail?: unknown;
}

export interface ImportPreview {
  resource: ImportResource;
  source: string;
  items: ImportPreviewItem[];
  warnings: string[];
  payload: unknown;
}

export interface DeeplinkImportResult {
  resource: ImportResource;
  imported: number;
  skipped: number;
}

export interface ConfigBackup {
  name: string;
  createdAt: number;
  verified: boolean;
  sourceName?: string | null;
}

export interface LibraryBackupInfo {
  archivePath: string;
  createdAt: number;
  entries: number;
}

export interface LibraryArchivePreview {
  archivePath: string;
  createdAt: number;
  schemaVersion: number;
  entries: number;
  totalBytes: number;
  credentialsIncluded: boolean;
}

export interface LibraryRestoreResult {
  archivePath: string;
  restoredEntries: number;
  backupDbPath?: string | null;
  restartRequired: boolean;
  credentialsImported: boolean;
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
  /** `~/.claude/agents` (Claude Code custom agents). */
  claudeAgentsPath: string;
  codexConfigDir: string;
  codexConfigPath: string;
  codexAuthPath: string;
  codexSkillsDir: string;
  codexPluginsCacheDir: string;
  codexSessionsDir: string;
  codexAgentsPath: string;
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

export interface DataRootInfo {
  activePath: string;
  legacyPath: string;
  migrated: boolean;
  restartRequired: boolean;
}

/** Basic database info returned by `get_db_info`. */
export interface DbInfo {
  path: string;
  schemaVersion: number;
  providerCount: number;
}

export interface DoctorCheck {
  id: string;
  label: string;
  ok: boolean;
  detail: string;
  repairAction?: string | null;
}

export interface DoctorReport {
  checks: DoctorCheck[];
}

export interface VisibilityRepairResult {
  codexProviderFiles: number;
  codexProviderRows: number;
  codexUsageInserted: number;
  claudeCodeUsageInserted: number;
  message: string;
}

export interface DoctorRepairResult {
  id: string;
  message: string;
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
  registryName: string;
  command?: string;
  taskManagerDisabled: boolean;
}

/** One unified MCP server definition. */
export interface McpServer {
  id: string;
  name: string;
  /** Raw JSON entry stored under `mcpServers.<name>`. */
  serverConfig: Record<string, unknown>;
  enabledClaudeCode: boolean;
  enabledClaudeDesktop: boolean;
  enabledCodex: boolean;
  sortIndex: number;
  createdAt: number;
}

/** Input shape for creating or updating an MCP server. */
export interface McpServerInput {
  id?: string;
  name: string;
  serverConfig: Record<string, unknown>;
  enabledClaudeCode: boolean;
  enabledClaudeDesktop: boolean;
  enabledCodex: boolean;
}

export type McpTarget = "claude_code" | "claude_desktop" | "codex";

export interface CodexAuthStatus {
  configPath: string;
  authPath: string;
  configExists: boolean;
  loggedIn: boolean;
  loginCommand: string;
}

export interface CodexOauthDeviceStart {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  interval: number;
  expiresIn: number;
}

export interface CodexOauthAccount {
  accountId: string;
  email: string;
  authenticatedAt: number;
}

export interface CodexOauthPollResult {
  status: "pending" | "complete" | "expired" | "denied" | "error";
  account?: CodexOauthAccount;
  message?: string;
}

export interface McpImportSummary {
  imported: number;
  updated: number;
}

export interface McpOauthStatus {
  storage: string;
  path?: string | null;
  serverNames: string[];
  entryCount: number;
  clearable: boolean;
  note?: string | null;
}

export interface McpDesktopConflictStatus {
  desktopInstalled: boolean;
  managedDesktopServers: number;
  liveDesktopServers: number;
  extensionArtifacts: string[];
  conflictLikely: boolean;
  message?: string | null;
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

export type PromptTarget = "claude_code" | "codex";

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
  descriptionZh?: string | null;
  source?: InstalledSkillSource | null;
}

export interface UnmanagedSkill {
  directory: string;
  name: string;
  description: string;
  foundIn: string[];
  path: string;
}

export type SkillTarget = "claude_code" | "codex";

export interface Agent {
  name: string;
  path: string;
  enabled: boolean;
  description: string;
}

export interface AgentDraft {
  name: string;
  description: string;
  body?: string;
}

export interface CodexPlugin {
  pluginId: string;
  name: string;
  marketplace: string;
  version?: string | null;
  enabled: boolean;
  installed: boolean;
  path?: string | null;
}

export interface CodexPluginsSnapshot {
  plugins: CodexPlugin[];
  configPath: string;
  cachePath: string;
  configPluginCount: number;
  cachePluginCount: number;
  parseOk: boolean;
  parseError?: string | null;
}

export interface CodexMarketplace {
  name: string;
  root?: string | null;
  source?: string | null;
  raw?: string | null;
}

export interface CodexMarketplaceListResult {
  marketplaces: CodexMarketplace[];
  rawOutput: string;
  usedJson: boolean;
}

export interface CodexPluginCommandResult {
  ok: boolean;
  message: string;
  stdout: string;
  stderr: string;
}

export type CodexWebSearchMode = "disabled" | "cached" | "indexed" | "live";

export interface CodexWebSearchSnapshot {
  mode: CodexWebSearchMode;
  configPath: string;
  setInConfig: boolean;
}

export interface EndpointSpeedtestResult {
  ok: boolean;
  latencyMs?: number | null;
  message: string;
  checkedAt: number;
  url: string;
}

export interface InstalledSkillSource {
  kind: "github" | "zip" | string;
  sourceUrl?: string | null;
  revision?: string | null;
  repositoryPath?: string | null;
  installedAt: number;
  contentSha256: string;
}

export interface SkillUpdateStatus {
  name: string;
  status: "untracked" | "unsupported" | "local_modified" | "up_to_date" | "update_available" | string;
  message: string;
  localModified: boolean;
  localRevision?: string | null;
  remoteRevision?: string | null;
}

export interface RepositorySkill {
  name: string;
  path: string;
  description: string;
}

export interface SkillRepositorySnapshot {
  repositoryUrl: string;
  fetchedAt?: number | null;
  revision?: string | null;
  skills: RepositorySkill[];
}

export interface CurrencyAmount {
  currency: string;
  amount: number;
}

export interface UsageSummary {
  requestCount: number;
  successfulRequestCount: number;
  inputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
  outputTokens: number;
  estimatedCost: number;
  estimatedCostCurrency: string;
  estimatedCostsByCurrency: CurrencyAmount[];
}

export interface UsageBreakdown {
  key: string;
  requestCount: number;
  inputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
  outputTokens: number;
  estimatedCost: number;
  currency: string;
}

export interface UsageTrendPoint {
  date: string;
  requestCount: number;
  inputTokens: number;
  cacheReadInputTokens: number;
  cacheCreationInputTokens: number;
  outputTokens: number;
  estimatedCost: number;
  currency: string;
}

export interface UsageDashboard {
  summary: UsageSummary;
  byProvider: UsageBreakdown[];
  byModel: UsageBreakdown[];
  trend: UsageTrendPoint[];
  trendGranularity: "hour" | "day";
  localCodex: LocalCodexUsage;
  localClaudeCode: LocalCodexUsage;
}

export interface LocalCodexUsage {
  available: boolean;
  sessionCount: number;
  eventCount: number;
  message: string;
}

export interface ModelPricing {
  model: string;
  provider: string;
  inputPricePerMillion: number;
  cacheReadPricePerMillion: number;
  cacheWritePricePerMillion: number;
  outputPricePerMillion: number;
  batchInputPricePerMillion: number;
  batchOutputPricePerMillion: number;
  currency: string;
  sourceUrl: string;
  effectiveDate: string;
  isDefault: boolean;
}

export interface ModelPricingInput {
  model: string;
  provider?: string;
  inputPricePerMillion: number;
  cacheReadPricePerMillion?: number;
  cacheWritePricePerMillion?: number;
  outputPricePerMillion: number;
  batchInputPricePerMillion?: number;
  batchOutputPricePerMillion?: number;
  currency: string;
}

export interface UpdateMirrorSettings {
  useMirror: boolean;
  mirrorBase: string;
}

export interface AppUpdateInfo {
  version: string;
}

export interface PricingImportPreview {
  newModels: string[];
  updatedModels: string[];
  errors: string[];
  validRows: number;
}

export interface PricingCatalog {
  version: string;
  entries: ModelPricing[];
}

export type SyncTargetKind = "wsl" | "ssh";
export type SyncItem = "provider_presets" | "mcp" | "prompts" | "skills" | "session_archives";

export interface PathMapping {
  windowsPath: string;
  remotePath: string;
}

export interface SyncTarget {
  id: string;
  name: string;
  kind: SyncTargetKind;
  wslDistribution?: string | null;
  sshHost?: string | null;
  sshPort?: number | null;
  remoteRoot: string;
  pathMappings: PathMapping[];
  items: SyncItem[];
  lastSyncedAt?: number | null;
}

export interface SyncPreviewChange {
  item: SyncItem;
  sourcePath: string;
  remotePath: string;
  status: string;
}

export interface SyncPreview {
  target: SyncTarget;
  changes: SyncPreviewChange[];
  warnings: string[];
}

export interface SyncPushResult {
  targetId: string;
  archivePath: string;
  remotePath: string;
  bytes: number;
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
  usageAvailable: boolean;
  durationMs: number;
  targetApp: string | null;
  protocol: string | null;
  route: string | null;
  isStream: boolean;
  errorCategory: string | null;
  diagnostic: string | null;
  dataSource: string;
  sessionId: string | null;
}

export interface PaginatedProxyLogs {
  data: ProxyRequestLog[];
  total: number;
  page: number;
  pageSize: number;
}

export interface ProxyLogListInput {
  days?: number;
  hours?: number;
  today?: boolean;
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

export interface CodexCliVersionInfo {
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
}

export interface NodeRuntimeStatus {
  installed: boolean;
  version: string | null;
  meetsMinimum: boolean;
  npmPath: string | null;
  nodePath: string | null;
  source: string;
  fnmInstalled: boolean;
  installHint: string;
}

export type SessionProvider = "claude_code" | "codex";

export interface SessionProviderStatus {
  provider: SessionProvider;
  status: "available" | "not_found" | "degraded";
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
  /** Codex thread pin from local SQLite index. */
  pinned?: boolean;
}

export interface SessionMessage {
  role: string;
  content: string;
  timestamp?: number;
}

export interface SessionScanResult {
  sessions: SessionMeta[];
  providers: SessionProviderStatus[];
  total: number;
  offset: number;
  limit?: number;
}

export interface SessionArchiveInfo {
  archivePath: string;
  sessionId: string;
  createdAt: number;
}

export interface SessionBatchBackupInfo {
  archives: SessionArchiveInfo[];
}

export interface SessionBatchExportInfo {
  archivePath: string;
  sessionCount: number;
  createdAt: number;
}

export interface ProfileScopePayload {
  providerId?: string | null;
  mcpIds: string[];
  skillIds: string[];
  agentIds?: string[];
  promptId?: string | null;
}

export interface ProfilePayload {
  claudeCode?: ProfileScopePayload | null;
  claudeDesktop?: ProfileScopePayload | null;
  codex?: ProfileScopePayload | null;
}

export interface Profile {
  id: string;
  name: string;
  payload: ProfilePayload;
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
}

export interface ProfileSnapshotScopes {
  claudeCode: boolean;
  claudeDesktop: boolean;
  codex: boolean;
}

export interface ApplyProfileResult {
  profile: Profile;
  warnings: string[];
}
