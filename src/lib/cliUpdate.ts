// Agent CLI 版本提示与 OpenCode 升级动作的纯函数。
// 注意：本文件不得有运行时 import（scripts/test-cli-update.mjs 用 transpile + vm 直接执行它）。

export type OpenCodeChannel = "v1" | "v2";

/** 产品只安装这一条。v1（opencode-ai）不再提供，官方会直接升到 v2。 */
export const OPENCODE_V2_PACKAGE = "@opencode/cli";

/** Claude Code / Codex / OpenCode / Pi 四种版本信息共有字段的最小结构。 */
export interface CliInfoLike {
  installed: boolean;
  currentVersion?: string | null;
  latestVersion?: string | null;
  updateAvailable?: boolean;
  channel?: string | null;
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

export function opencodePackageName(channel: OpenCodeChannel): string {
  return channel === "v1" ? "opencode-ai" : OPENCODE_V2_PACKAGE;
}

/**
 * 安装与升级只走 v2。已装 v1 时升级会卸载 opencode-ai 再装 @opencode/cli，需要确认。
 * 版本比较由后端对 v2 最新版完成，这里不再做通道选择。
 */
export function getOpenCodeAction(info: CliInfoLike | null | undefined): OpenCodeAction {
  if (!info?.installed) {
    return {
      kind: "install",
      channel: "v2",
      needsConfirm: false,
      labelKey: "agentHub.install",
    };
  }
  if (info.channel === "v1") {
    return {
      kind: "switch",
      channel: "v2",
      needsConfirm: true,
      labelKey: "agentHub.upgrade",
    };
  }
  return { kind: "upgrade", channel: "v2", needsConfirm: false, labelKey: "agentHub.upgrade" };
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
