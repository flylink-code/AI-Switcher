import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";
import ts from "typescript";

// 转译并执行真实 Hook 源码；仅替换 React 生命周期与 Tauri/DOM 边界。
const source = fs.readFileSync(new URL("../src/lib/useUsageLogRefresh.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
const handlers = new Set();
const visibilityHandlers = new Set();
let visibilityState = "visible";
let registerCount = 0;
let unregisterCount = 0;
let current;
const sameDeps = (a, b) => a && b && a.length === b.length && a.every((v, i) => Object.is(v, b[i]));
const react = {
  useRef(value) {
    const index = current.cursor++;
    return current.slots[index] ??= { current: value };
  },
  useEffect(effect, deps) {
    const index = current.cursor++;
    const old = current.slots[index];
    if (!sameDeps(old?.deps, deps)) {
      current.effects.push(() => {
        old?.cleanup?.();
        current.slots[index] = { deps, cleanup: effect() };
      });
    }
  },
};
const exports = {};
vm.runInNewContext(js, {
  exports, setTimeout, clearTimeout, setInterval, clearInterval, Date,
  document: {
    get visibilityState() { return visibilityState; },
    addEventListener(name, fn) { assert.equal(name, "visibilitychange"); visibilityHandlers.add(fn); },
    removeEventListener(name, fn) { assert.equal(name, "visibilitychange"); visibilityHandlers.delete(fn); },
  },
  require(name) {
    if (name === "react") return react;
    if (name === "@tauri-apps/api/event") return { listen: async (event, handler) => {
      assert.equal(event, "usage-log-recorded");
      registerCount++;
      handlers.add(handler);
      return () => { unregisterCount++; handlers.delete(handler); };
    } };
    throw new Error(`Unexpected import ${name}`);
  },
});
function instance() {
  const state = { slots: [], cursor: 0, effects: [] };
  return {
    render(options) {
      current = state;
      state.cursor = 0;
      exports.useUsageLogRefresh(options);
      for (const effect of state.effects.splice(0)) effect();
    },
    unmount() {
      for (const slot of state.slots) slot?.cleanup?.();
      state.slots = [];
    },
  };
}
const wait = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms));
const emit = () => { for (const fn of handlers) fn(); };
const visible = (state) => {
  visibilityState = state;
  for (const fn of visibilityHandlers) fn();
};

const strict = instance();
strict.render({ onRefresh() {} });
strict.unmount();
strict.render({ onRefresh() {} });
await wait();
assert.equal(handlers.size, 1);
strict.unmount();
assert.equal(handlers.size, 0);

let count = 0;
let release;
const a = instance();
const callback = () => {
  count++;
  if (count === 1) return new Promise((resolve) => { release = resolve; });
};
a.render({ onRefresh: callback, throttleMs: 20 });
await wait();
emit();
await wait(5);
for (let i = 0; i < 5; i++) emit();
assert.equal(count, 1);
release();
await wait(45);
assert.equal(count, 2);

visible("hidden");
emit();
await wait(25);
assert.equal(count, 2);
visible("visible");
await wait(25);
assert.equal(count, 3);

// 即使隐藏期间没有事件，恢复可见也补查。
visible("hidden");
visible("visible");
await wait(25);
assert.equal(count, 4);

const b = instance();
b.render({ onRefresh() {}, throttleMs: 20 });
await wait();
assert.equal(handlers.size, 1, "组件共用一个 Tauri listener");
b.unmount();
a.render({ onRefresh: callback, throttleMs: 20, enabled: false });
emit();
await wait(25);
assert.equal(count, 4);
assert.equal(handlers.size, 0);
a.unmount();

// 旧订阅仍在途时启停，新订阅独立完成尾随刷新。
let calls = 0;
let finishOld;
const c = instance();
const pending = () => {
  calls++;
  if (calls === 1) return new Promise((resolve) => { finishOld = resolve; });
};
c.render({ onRefresh: pending, throttleMs: 10 });
await wait();
emit();
await wait(5);
c.render({ onRefresh: pending, throttleMs: 10, enabled: false });
c.render({ onRefresh: pending, throttleMs: 10 });
await wait();
emit();
await wait(5);
finishOld();
emit();
await wait(25);
assert.equal(calls, 3);
c.unmount();
await wait();
assert.equal(handlers.size, 0);
assert.equal(visibilityHandlers.size, 0);
assert.equal(registerCount, unregisterCount);
// 外部查询在途时先复用，再补查事件后的状态，不取消在途查询。
let queryCalls = 0;
let finishQuery;
const oldQuery = new Promise((resolve) => { finishQuery = resolve; });
const refresh = exports.refreshUsageQuery({
  isFetching: true,
  refetch(options) {
    assert.equal(options.cancelRefetch, false);
    queryCalls++;
    return queryCalls === 1 ? oldQuery : Promise.resolve("latest");
  },
});
assert.equal(queryCalls, 1);
finishQuery("old");
assert.equal(await refresh, "latest");
assert.equal(queryCalls, 2);

console.log("PASS: 真实 Hook 源码的 StrictMode、共享订阅、节流尾随、可见性补查、启停、旧异步隔离与外部在途补查");
