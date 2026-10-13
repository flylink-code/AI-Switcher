import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

// 转译并执行真实 src/lib/cliUpdate.ts（该文件不得有运行时 import）。
const source = fs.readFileSync(new URL("../src/lib/cliUpdate.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
const exports = {};
vm.runInNewContext(js, { exports });
const {
  getToolUpdateState,
  getUpgradeButton,
  resolveOpenCodeChannel,
  getOpenCodeUpdateState,
  getOpenCodeAction,
  opencodePackageName,
  isCodexCliMissing,
} = exports;

// ---- Task 2：四种 info 形状（Claude Code / Codex / Pi 无 channel；OpenCode 有）----
const base = { installed: true, currentVersion: "1.0.0", latestVersion: "1.1.0", updateAvailable: true };
for (const shape of [
  { ...base },
  { ...base, wslDistro: null },
  { ...base, channel: "v2", v1LatestVersion: "0.9.0", v2LatestVersion: "1.1.0" },
]) {
  const state = getToolUpdateState(shape);
  assert.deepEqual({ ...state }, {
    installed: true, currentVersion: "1.0.0", latestVersion: "1.1.0", hasUpdate: true, latestKnown: true,
  });
  assert.deepEqual({ ...getUpgradeButton(state) }, { type: "primary", disabled: false, labelKey: "upgrade" });
}
// 相同 / current 更高：后端 updateAvailable=false → 已是最新（禁用）
const same = getToolUpdateState({ ...base, latestVersion: "1.0.0", updateAvailable: false });
assert.equal(same.hasUpdate, false);
assert.equal(same.latestKnown, true);
assert.deepEqual({ ...getUpgradeButton(same) }, { type: "default", disabled: true, labelKey: "upToDate" });
const higher = getToolUpdateState({ ...base, currentVersion: "2.0.0", updateAvailable: false });
assert.equal(higher.hasUpdate, false);
assert.equal(getUpgradeButton(higher).disabled, true);
// latest 未知（离线）：不显示徽标，升级仍可点，且不是“已是最新”
const offline = getToolUpdateState({ ...base, latestVersion: null, updateAvailable: false });
assert.equal(offline.latestKnown, false);
assert.equal(offline.hasUpdate, false);
assert.deepEqual({ ...getUpgradeButton(offline) }, { type: "default", disabled: false, labelKey: "upgrade" });
// 即使 updateAvailable=true，latest 未知也不算有更新
assert.equal(getToolUpdateState({ ...base, latestVersion: null }).hasUpdate, false);
// 未安装 / info 为空
const missing = getToolUpdateState({ installed: false, currentVersion: null, latestVersion: "1.1.0", updateAvailable: true });
assert.equal(missing.hasUpdate, false);
assert.equal(getUpgradeButton(missing).disabled, false);
for (const empty of [undefined, null]) {
  const state = getToolUpdateState(empty);
  assert.deepEqual({ ...state }, {
    installed: false, currentVersion: null, latestVersion: null, hasUpdate: false, latestKnown: false,
  });
}

// ---- Task 3：OpenCode 通道 ----
const oc = (channel, extra = {}) => ({
  installed: true, currentVersion: "1.0.0", channel,
  latestVersion: channel === "v1" ? "1.5.0" : "2.0.0",
  v1LatestVersion: "1.5.0", v2LatestVersion: "2.0.0", updateAvailable: true, ...extra,
});
const notInstalled = { installed: false, currentVersion: null, channel: null, latestVersion: null, updateAvailable: false };

assert.equal(opencodePackageName("v1"), "opencode-ai");
assert.equal(opencodePackageName("v2"), "@opencode/cli");

// resolveOpenCodeChannel：跟随检测，override 优先，未安装默认 v2
assert.equal(resolveOpenCodeChannel(notInstalled, null), "v2");
assert.equal(resolveOpenCodeChannel(undefined, null), "v2");
assert.equal(resolveOpenCodeChannel(oc("v1"), null), "v1");
assert.equal(resolveOpenCodeChannel(oc("v2"), null), "v2");
assert.equal(resolveOpenCodeChannel(oc("v1"), "v2"), "v2");
assert.equal(resolveOpenCodeChannel(oc(null), null), "v2");

// 未安装：install，不需确认
for (const [sel, key] of [["v1", "about.runOpenCodeInstallV1"], ["v2", "about.runOpenCodeInstallV2"]]) {
  assert.deepEqual({ ...getOpenCodeAction(notInstalled, sel) }, {
    kind: "install", channel: sel, needsConfirm: false, labelKey: key,
  });
}
// 装 v1：选 v1 → upgrade；选 v2 → switch（需确认）
assert.deepEqual({ ...getOpenCodeAction(oc("v1"), "v1") }, {
  kind: "upgrade", channel: "v1", needsConfirm: false, labelKey: "agentHub.upgrade",
});
assert.deepEqual({ ...getOpenCodeAction(oc("v1"), "v2") }, {
  kind: "switch", channel: "v2", needsConfirm: true, labelKey: "about.runOpenCodeSwitchV2",
});
// 装 v2：选 v2 → upgrade；选 v1 → switch
assert.deepEqual({ ...getOpenCodeAction(oc("v2"), "v2") }, {
  kind: "upgrade", channel: "v2", needsConfirm: false, labelKey: "agentHub.upgrade",
});
assert.deepEqual({ ...getOpenCodeAction(oc("v2"), "v1") }, {
  kind: "switch", channel: "v1", needsConfirm: true, labelKey: "about.runOpenCodeSwitchV1",
});
// 通道未知（已安装但 channel=null）：不触发切换确认
assert.equal(getOpenCodeAction(oc(null), "v1").kind, "upgrade");

// 更新状态：同通道才比较；跨通道不显示更新，latest 取所选线
const sameLine = getOpenCodeUpdateState(oc("v2"), "v2");
assert.equal(sameLine.hasUpdate, true);
assert.equal(sameLine.latestVersion, "2.0.0");
const crossLine = getOpenCodeUpdateState(oc("v2"), "v1");
assert.equal(crossLine.hasUpdate, false);
assert.equal(crossLine.latestVersion, "1.5.0");
assert.equal(crossLine.latestKnown, true);
const crossUnknown = getOpenCodeUpdateState(oc("v2", { v1LatestVersion: null }), "v1");
assert.equal(crossUnknown.latestKnown, false);
assert.equal(crossUnknown.hasUpdate, false);
// 同通道最新版本为 null 时回退所选线
const fallback = getOpenCodeUpdateState(oc("v1", { latestVersion: null, updateAvailable: false }), "v1");
assert.equal(fallback.latestVersion, "1.5.0");
assert.equal(fallback.hasUpdate, false);
// 未安装：退化为 getToolUpdateState
assert.equal(getOpenCodeUpdateState(notInstalled, "v2").hasUpdate, false);
assert.equal(getOpenCodeUpdateState(undefined, "v1").installed, false);

// ---- Task 5：isCodexCliMissing ----
assert.equal(isCodexCliMissing("CODEX_CLI_MISSING: 无法启动 Codex CLI"), true);
assert.equal(isCodexCliMissing(new Error("CODEX_CLI_MISSING: x")), true);
assert.equal(isCodexCliMissing({ message: "CODEX_CLI_MISSING: x" }), true);
assert.equal(isCodexCliMissing("无法启动 Codex CLI（请确认已安装并在 PATH 中）: boom"), false);
assert.equal(isCodexCliMissing(new Error("other")), false);
assert.equal(isCodexCliMissing({ message: 42 }), false);
assert.equal(isCodexCliMissing(null), false);
assert.equal(isCodexCliMissing(undefined), false);

console.log("cli-update tests passed");
