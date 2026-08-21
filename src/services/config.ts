import { call } from "./ipc";
import type {
  ApplyProfileResult,
  ConfigBackup,
  LibraryArchivePreview,
  LibraryBackupInfo,
  LibraryRestoreResult,
  Profile,
  ProfilePayload,
  ProfileSnapshotScopes,
  ProviderTarget,
  SyncPreview,
  SyncPushResult,
  SyncTarget,
} from "@/types/backend";

export async function exportLibraryBackup(
  destinationDir?: string | null,
  includeCredentials = false,
): Promise<LibraryBackupInfo> {
  return call<LibraryBackupInfo>("export_library_backup", {
    destinationDir: destinationDir?.trim() ? destinationDir : null,
    includeCredentials,
  });
}

export async function previewLibraryBackup(archivePath: string): Promise<LibraryArchivePreview> {
  return call<LibraryArchivePreview>("preview_library_backup", { archivePath });
}

export async function restoreLibraryBackup(archivePath: string): Promise<LibraryRestoreResult> {
  return call<LibraryRestoreResult>("restore_library_backup", { archivePath });
}

export async function findLatestLibraryArchive(directory: string): Promise<string> {
  return call<string>("find_latest_library_archive_cmd", { directory });
}

// ---- Cross-environment sync -------------------------------------------------

export async function listSyncTargets(): Promise<SyncTarget[]> {
  return call<SyncTarget[]>("list_sync_targets", {});
}

export async function saveSyncTarget(target: SyncTarget): Promise<SyncTarget> {
  return call<SyncTarget>("save_sync_target", { target });
}

export async function deleteSyncTarget(id: string): Promise<void> {
  return call<void>("delete_sync_target", { id });
}

export async function discoverWslDistributions(): Promise<string[]> {
  return call<string[]>("discover_wsl_distributions", {});
}

export async function previewSync(targetId: string): Promise<SyncPreview> {
  return call<SyncPreview>("preview_sync", { targetId });
}

export async function pushSyncArchive(
  targetId: string,
  password?: string | null,
  includeApiKeys = false,
): Promise<SyncPushResult> {
  return call<SyncPushResult>("push_sync_archive", {
    targetId,
    password: password?.trim() ? password : null,
    includeApiKeys,
  });
}

export async function getWslRuntimeStatus(): Promise<import("@/types/backend").WslRuntimeStatus> {
  return call("get_wsl_runtime_status", {});
}

export async function syncWslDirect(): Promise<import("@/types/backend").WslRuntimeStatus> {
  return call("sync_wsl_direct", {});
}

export async function getWebDavSettings(): Promise<import("@/types/backend").WebDavSettings> {
  return call("get_webdav_settings", {});
}

export async function setWebDavSettings(input: {
  url: string;
  username: string;
  remotePath: string;
  password?: string | null;
}): Promise<import("@/types/backend").WebDavSettings> {
  return call("set_webdav_settings", input);
}

export async function uploadLibraryToWebDav(includeCredentials = false): Promise<string> {
  return call("upload_library_to_webdav", { includeCredentials });
}

export async function restoreLibraryFromWebDav(): Promise<LibraryRestoreResult> {
  return call("restore_library_from_webdav", {});
}

// ---- Providers -------------------------------------------------------------

export async function listConfigBackups(
  target: ProviderTarget,
  directory?: string | null,
): Promise<ConfigBackup[]> {
  return call<ConfigBackup[]>("list_config_backups", {
    target,
    directory: directory?.trim() ? directory : null,
  });
}

export async function previewConfigBackup(
  target: ProviderTarget,
  name: string,
  directory?: string | null,
): Promise<string> {
  return call<string>("preview_config_backup", {
    target,
    name,
    directory: directory?.trim() ? directory : null,
  });
}

export async function restoreConfigBackup(
  target: ProviderTarget,
  name: string,
  directory?: string | null,
): Promise<void> {
  return call<void>("restore_config_backup", {
    target,
    name,
    directory: directory?.trim() ? directory : null,
  });
}

// ---- Local proxy ------------------------------------------------------------

export async function listProfiles(): Promise<Profile[]> {
  return call<Profile[]>("list_profiles", {});
}

export async function getCurrentProfileId(): Promise<string | null> {
  return call<string | null>("get_current_profile_id", {});
}

export async function createProfile(
  name: string,
  scopes: ProfileSnapshotScopes,
): Promise<Profile> {
  return call<Profile>("create_workspace_profile", { name, scopes });
}

export async function updateProfile(
  id: string,
  name?: string,
  payload?: ProfilePayload,
): Promise<Profile> {
  return call<Profile>("update_workspace_profile", { id, name, payload });
}

export async function deleteProfile(id: string): Promise<void> {
  return call<void>("delete_workspace_profile", { id });
}

export async function applyProfile(
  id: string,
  autosavePrevious = true,
): Promise<ApplyProfileResult> {
  return call<ApplyProfileResult>("apply_profile", { id, autosavePrevious });
}
