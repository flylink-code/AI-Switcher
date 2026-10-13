// Agent CLI 版本提示与 OpenCode 通道决策的纯函数。
// 注意：本文件不得有运行时 import（scripts/test-cli-update.mjs 用 transpile + vm 直接执行它）。

export type OpenCodeChannel = "v1" | "v2";

/** Claude Code / Codex / OpenCode / Pi 四种版本信息共有字段的最小结构。 */
export interface CliInfoLike {
  installed: boolean;
  currentVersion?: string | null;
  latestVersion?: string | null;
  updateAvailable?: boolean;
  channel?: string | null;
  v1LatestVersion?: string | null;
  v2LatestVersion?: string | null;
}

export interface ToolUpdateState {
  installed: boolean;
  currentVersion: string | null;
  latestVersion: string | null;
  hasUpdate: boolean;
  /** 已拿到最新版本号；false 表示离线/未知，区别于“已是最新”。 */
  latestKnown: boolean;
}

export interface UpgradeButtonState {
  type: "primary" | "default";
  disabled: boolean;
  labelKey: "upgrade" | "upToDate";
}

export interface OpenCodeAction {
  kind: "install" | "switch" | "upgrade";
  channel: OpenCodeChannel;
  needsConfirm: boolean;
  /** i18n key（页面里用 t() 取文案）。 */
  labelKey: string;
}

export function getToolUpdateState(info: CliInfoLike | null | undefined): ToolUpdateState {
  const installed = !!info?.installed;
  const latestVersion = info?.latestVersion ?? null;
  const latestKnown = latestVersion != null;
  return {
    installed,
    currentVersion: info?.currentVersion ?? null,
    latestVersion,
    // 数值比较由后端 update_available 完成，这里只信任它。
    hasUpdate: installed && latestKnown && info?.updateAvailable === true,
    latestKnown,
  };
}

export function getUpgradeButton(state: ToolUpdateState): UpgradeButtonState {
  if (state.hasUpdate) return { type: "primary", disabled: false, labelKey: "upgrade" };
  if (state.installed && state.latestKnown) return { type: "default", disabled: true, labelKey: "upToDate" };
  return { type: "default", disabled: false, labelKey: "upgrade" };
}

export function isOpenCodeChannel(value: string | null | undefined): value is OpenCodeChannel {
  return value === "v1" || value === "v2";
}

export function opencodePackageName(channel: OpenCodeChannel): string {
  return channel === "v1" ? "opencode-ai" : "@opencode/cli";
}

/** 未手动选择时跟随检测到的通道；未安装默认 v2。 */
export function resolveOpenCodeChannel(
  info: CliInfoLike | null | undefined,
  override: OpenCodeChannel | null | undefined,
): OpenCodeChannel {
  if (override) return override;
  return isOpenCodeChannel(info?.channel) ? info.channel : "v2";
}

/** 仅当所选通道 = 检测通道时才比较更新；跨通道时 hasUpdate 恒为 false。 */
export function getOpenCodeUpdateState(
  info: CliInfoLike | null | undefined,
  selected: OpenCodeChannel,
): ToolUpdateState {
  const base = getToolUpdateState(info);
  if (!base.installed || !isOpenCodeChannel(info?.channel)) return base;
  if (info.channel === selected) {
    const latestVersion = info.latestVersion ?? (selected === "v1" ? info.v1LatestVersion : info.v2LatestVersion) ?? null;
    return { ...base, latestVersion, latestKnown: latestVersion != null, hasUpdate: base.hasUpdate };
  }
  const latestVersion = (selected === "v1" ? info.v1LatestVersion : info.v2LatestVersion) ?? null;
  return { ...base, latestVersion, latestKnown: latestVersion != null, hasUpdate: false };
}

export function getOpenCodeAction(
  info: CliInfoLike | null | undefined,
  selected: OpenCodeChannel,
): OpenCodeAction {
  if (!info?.installed) {
    return {
      kind: "install",
      channel: selected,
      needsConfirm: false,
      labelKey: selected === "v2" ? "about.runOpenCodeInstallV2" : "about.runOpenCodeInstallV1",
    };
  }
  if (isOpenCodeChannel(info.channel) && info.channel !== selected) {
    return {
      kind: "switch",
      channel: selected,
      needsConfirm: true,
      labelKey: selected === "v2" ? "about.runOpenCodeSwitchV2" : "about.runOpenCodeSwitchV1",
    };
  }
  return { kind: "upgrade", channel: selected, needsConfirm: false, labelKey: "agentHub.upgrade" };
}

/** 后端 run_codex_cli* 在未安装 Codex CLI 时返回的稳定前缀。 */
const CODEX_CLI_MISSING_PREFIX = "CODEX_CLI_MISSING:";

export function isCodexCliMissing(error: unknown): boolean {
  let text = "";
  if (typeof error === "string") {
    text = error;
  } else if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") text = message;
  }
  return text.includes(CODEX_CLI_MISSING_PREFIX);
}
