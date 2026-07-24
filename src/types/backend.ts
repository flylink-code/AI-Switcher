/**
 * Shared types for values exchanged with the Rust backend over Tauri IPC.
 * Keep these in sync with the corresponding serde structs in src-tauri/src/commands/.
 */

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
