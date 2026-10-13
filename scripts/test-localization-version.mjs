import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

// 转译并执行真实 src/lib/localizationVersion.ts（该文件不得有运行时 import）。
const source = fs.readFileSync(new URL("../src/lib/localizationVersion.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
const exports = {};
vm.runInNewContext(js, { exports });
const { normalizeResourceVersion, compareResourceVersion } = exports;

// normalize：去空白、去 v 前缀、空值 → null
assert.equal(normalizeResourceVersion(" v1.2.3 "), "1.2.3");
assert.equal(normalizeResourceVersion("V2.0"), "2.0");
assert.equal(normalizeResourceVersion("  "), null);
assert.equal(normalizeResourceVersion(null), null);
assert.equal(normalizeResourceVersion(undefined), null);

// compare：v 前缀不影响相同判断
assert.equal(compareResourceVersion("v1.2.3", "1.2.3"), "same");
// 远端更新
assert.equal(compareResourceVersion("1.2.3", "1.2.4"), "newer");
assert.equal(compareResourceVersion("1.9.0", "1.10.0"), "newer");
assert.equal(compareResourceVersion("1.2", "1.2.1"), "newer");
// 远端更旧 → same（不提示更新）
assert.equal(compareResourceVersion("1.3.0", "1.2.9"), "same");
// 无远端 → unknown
assert.equal(compareResourceVersion("1.0.0", null), "unknown");
assert.equal(compareResourceVersion("1.0.0", ""), "unknown");
// 无本地 → newer
assert.equal(compareResourceVersion(null, "1.0.0"), "newer");
assert.equal(compareResourceVersion("", "v1.0.0"), "newer");
// 非数字 → unknown
assert.equal(compareResourceVersion("abc", "1.0.0"), "unknown");
assert.equal(compareResourceVersion("1.0.0", "beta"), "unknown");

console.log("localization version tests passed");
