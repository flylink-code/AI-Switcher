import { call } from "./ipc";
import type {
  CodexProviderSyncResult,
  SessionArchiveInfo,
  SessionBatchBackupInfo,
  SessionBatchExportInfo,
  SessionMessage,
  SessionMeta,
  SessionProvider,
  SessionScanResult,
} from "@/types/backend";

export async function scanSessions(
  provider?: SessionProvider,
  offset?: number,
  limit?: number,
): Promise<SessionScanResult> {
  return call<SessionScanResult>("scan_sessions", {
    provider,
    offset: offset ?? null,
    limit: limit ?? null,
  });
}

export async function syncCodexSessionProviders(
  targetProvider?: string,
): Promise<CodexProviderSyncResult> {
  return call<CodexProviderSyncResult>("sync_codex_session_providers", {
    targetProvider: targetProvider ?? null,
  });
}

export async function searchSessionContents(
  query: string,
  provider?: SessionProvider,
  limit = 200,
): Promise<SessionScanResult> {
  return call<SessionScanResult>("search_session_contents", {
    query,
    provider,
    limit,
  });
}

export async function loadSessionMessages(
  provider: SessionProvider,
  sourcePath: string,
): Promise<SessionMessage[]> {
  return call<SessionMessage[]>("load_session_messages", {
    provider,
    sourcePath,
  });
}

export async function exportSession(provider: SessionProvider, sourcePath: string, destinationDir?: string): Promise<SessionArchiveInfo> {
  return call<SessionArchiveInfo>("export_session", { provider, sourcePath, destinationDir });
}

export async function exportSessionMarkdown(provider: SessionProvider, sourcePath: string, destinationDir?: string): Promise<string> {
  return call<string>("export_session_markdown", { provider, sourcePath, destinationDir });
}

export async function backupSessions(provider: SessionProvider, sourcePaths: string[]): Promise<SessionBatchBackupInfo> {
  return call<SessionBatchBackupInfo>("backup_sessions", { provider, sourcePaths });
}

export async function exportSessions(provider: SessionProvider, sourcePaths: string[], destinationDir?: string): Promise<SessionBatchExportInfo> {
  return call<SessionBatchExportInfo>("export_sessions", { provider, sourcePaths, destinationDir });
}

export async function importSession(provider: SessionProvider, archivePath: string): Promise<SessionMeta> {
  return call<SessionMeta>("import_session", { provider, archivePath });
}

export async function trashSession(provider: SessionProvider, sourcePath: string): Promise<SessionArchiveInfo> {
  return call<SessionArchiveInfo>("trash_session", { provider, sourcePath });
}

export async function restoreTrashedSession(provider: SessionProvider, archivePath: string): Promise<SessionMeta> {
  return call<SessionMeta>("restore_trashed_session", { provider, archivePath });
}

export async function listTrashedSessions(provider: SessionProvider): Promise<SessionArchiveInfo[]> {
  return call<SessionArchiveInfo[]>("list_trashed_sessions", { provider });
}
