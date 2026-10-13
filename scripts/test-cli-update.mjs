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

// ---- Task 3：OpenCode 只走 v2；已装 v1 时升级需确认 ----
const oc = (channel, extra = {}) => ({
  installed: true, currentVersion: "1.0.0", channel,
  latestVersion: "2.0.0", updateAvailable: true, ...extra,
});
const notInstalled = { installed: false, currentVersion: null, channel: null, latestVersion: null, updateAvailable: false };

assert.equal(opencodePackageName("v1"), "opencode-ai");
assert.equal(opencodePackageName("v2"), "@opencode/cli");

assert.deepEqual({ ...getOpenCodeAction(notInstalled) }, {
  kind: "install", channel: "v2", needsConfirm: false, labelKey: "agentHub.install",
});
assert.deepEqual({ ...getOpenCodeAction(undefined) }, {
  kind: "install", channel: "v2", needsConfirm: false, labelKey: "agentHub.install",
});
assert.deepEqual({ ...getOpenCodeAction(oc("v1")) }, {
  kind: "switch", channel: "v2", needsConfirm: true, labelKey: "agentHub.upgrade",
});
assert.deepEqual({ ...getOpenCodeAction(oc("v2")) }, {
  kind: "upgrade", channel: "v2", needsConfirm: false, labelKey: "agentHub.upgrade",
});
assert.equal(getOpenCodeAction(oc(null)).kind, "upgrade");
assert.equal(getOpenCodeAction(oc(null)).needsConfirm, false);

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
