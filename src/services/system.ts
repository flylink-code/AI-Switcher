import { call } from "./ipc";
import type {
  AppUpdateInfo,
  AutostartConfig,
  AutostartMode,
  CloseBehavior,
  DataRootInfo,
  DbInfo,
  DesktopLocalizationActionResult,
  DesktopLocalizationPackInfo,
  DesktopLocalizationPackValidation,
  DesktopLocalizationStatus,
  DoctorRepairResult,
  DoctorReport,
  LocalizationHubStatus,
  LocalizationUpstreamStatus,
  PathsInfo,
  UpdateMirrorSettings,
  VisibilityRepairResult,
} from "@/types/backend";

export async function ping(): Promise<string> {
  return call<string>("ping", {});
}

export async function restartApp(): Promise<void> {
  return call<void>("restart_app", {});
}

export async function getPaths(): Promise<PathsInfo> {
  return call<PathsInfo>("get_paths", {});
}

export async function getDbInfo(): Promise<DbInfo> {
  return call<DbInfo>("get_db_info", {});
}

export async function runEnvironmentDoctor(): Promise<DoctorReport> {
  return call<DoctorReport>("run_environment_doctor", {});
}

export async function repairEnvironmentVisibility(): Promise<VisibilityRepairResult> {
  return call<VisibilityRepairResult>("repair_environment_visibility", {});
}

export async function repairDoctorCheck(id: string): Promise<DoctorRepairResult> {
  return call<DoctorRepairResult>("repair_doctor_check", { id });
}

export async function getDataRoot(): Promise<DataRootInfo> {
  return call<DataRootInfo>("get_data_root", {});
}

export async function migrateDataRoot(targetPath: string): Promise<DataRootInfo> {
  return call<DataRootInfo>("migrate_data_root", { targetPath });
}

export async function backupNow(): Promise<string> {
  return call<string>("backup_now", {});
}

export async function getAutostartEnabled(): Promise<boolean> {
  return call<boolean>("get_autostart_enabled", {});
}

export async function setAutostartEnabled(enabled: boolean): Promise<void> {
  return call<void>("set_autostart_enabled", { enabled });
}

export async function getAutostartConfig(): Promise<AutostartConfig> {
  return call<AutostartConfig>("get_autostart_config", {});
}

export async function setAutostartConfig(mode: AutostartMode): Promise<void> {
  return call<void>("set_autostart_config", { mode });
}

export async function getCloseBehavior(): Promise<CloseBehavior> {
  return call<CloseBehavior>("get_close_behavior", {});
}

export async function setCloseBehavior(behavior: CloseBehavior): Promise<void> {
  return call<void>("set_close_behavior", { behavior });
}

export async function resolveCloseRequest(
  action: Exclude<CloseBehavior, "ask">,
  remember: boolean,
): Promise<void> {
  return call<void>("resolve_close_request", { action, remember });
}

export async function reportFrontendStartup(
  durationMs: number,
  reason: "completed" | "timeout" | "skipped",
  failures: string[],
): Promise<void> {
  return call<void>("report_frontend_startup", { durationMs, reason, failures });
}

export async function reportFrontendPerformance(
  kind: string,
  name: string,
  durationMs: number,
): Promise<void> {
  return call<void>("report_frontend_performance", {
    kind,
    name,
    durationMs: Math.max(0, Math.round(durationMs)),
  });
}

export async function getDesktopLocalizationStatus(): Promise<DesktopLocalizationStatus> {
  return call<DesktopLocalizationStatus>("get_desktop_localization_status", {});
}

export async function getLocalizationHubStatus(): Promise<LocalizationHubStatus> {
  return call<LocalizationHubStatus>("get_localization_hub_status", {});
}

export async function checkLocalizationUpstream(): Promise<LocalizationUpstreamStatus> {
  return call<LocalizationUpstreamStatus>("check_localization_upstream", {});
}

export async function installClaudeCodeLocalization(): Promise<string> {
  return call<string>("install_claude_code_localization", {});
}

export async function installEditorLocalizationHelper(editor: "vscode" | "cursor"): Promise<string> {
  return call<string>("install_editor_localization_helper", { editor });
}

export async function downloadDesktopLocalizationPack(): Promise<DesktopLocalizationPackInfo> {
  return call<DesktopLocalizationPackInfo>("download_desktop_localization_pack", {});
}

export async function selectDesktopLocalizationPack(): Promise<string | null> {
  return call<string | null>("select_desktop_localization_pack", {});
}

export async function validateDesktopLocalizationPack(
  path: string,
): Promise<DesktopLocalizationPackValidation> {
  return call<DesktopLocalizationPackValidation>(
    "validate_desktop_localization_pack",
    { path },
  );
}

export async function installDesktopLocalization(
  packPath: string,
): Promise<DesktopLocalizationActionResult> {
  return call<DesktopLocalizationActionResult>("install_desktop_localization", {
    packPath,
  });
}

export async function restoreDesktopLocalization(): Promise<DesktopLocalizationActionResult> {
  return call<DesktopLocalizationActionResult>("restore_desktop_localization", {});
}

// ---- Usage ------------------------------------------------------------------

export async function setAppLanguage(language: "zh-CN" | "en-US"): Promise<void> {
  return call<void>("set_app_language", { language });
}

export async function getUpdateMirrorSettings(): Promise<UpdateMirrorSettings> {
  return call<UpdateMirrorSettings>("get_update_mirror_settings", {});
}

export async function setUpdateMirrorSettings(settings: UpdateMirrorSettings): Promise<UpdateMirrorSettings> {
  return call<UpdateMirrorSettings>("set_update_mirror_settings", { settings });
}

export async function checkAppUpdate(): Promise<AppUpdateInfo | null> {
  return call<AppUpdateInfo | null>("check_app_update", {});
}

export async function installAppUpdate(version: string): Promise<void> {
  return call<void>("install_app_update", { version });
}

export async function getDismissedOnboardingTips(): Promise<string[]> {
  return call<string[]>("get_dismissed_onboarding_tips", {});
}

export async function dismissOnboardingTip(tipKey: string): Promise<void> {
  return call<void>("dismiss_onboarding_tip", { tipKey });
}

export async function restoreOnboardingTips(): Promise<void> {
  return call<void>("restore_onboarding_tips", {});
}
