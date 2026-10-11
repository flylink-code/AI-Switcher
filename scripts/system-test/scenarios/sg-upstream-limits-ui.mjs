import fs from "node:fs/promises";
import path from "node:path";
import { evaluate, invoke, withCdp, reloadPage } from "../cdp-invoke.mjs";
import { assert, pathUnderHome, providerInput, upsertUpstream } from "../lib.mjs";

export const id = "SG-upstream-limits-ui";
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(expression) {
  for (let i = 0; i < 80; i++) {
    if (await evaluate(expression)) return;
    await wait(150);
  }
  const snapshot = await evaluate(`(() => {
    const rows = Array.from(document.querySelectorAll('tr')).slice(0, 8).map((row) => row.innerText.replace(/\\s+/g, ' ').slice(0, 160));
    const buttons = Array.from(document.querySelectorAll('button')).slice(0, 40).map((button) => {
      const label = button.getAttribute('aria-label') || '';
      const text = (button.innerText || '').replace(/\\s+/g, ' ').trim().slice(0, 40);
      return label ? label + '|' + text : text;
    });
    return JSON.stringify({ rows, buttons });
  })()`).catch((error) => String(error));
  throw new Error(`DOM 等待超时：${expression}\n${snapshot}`);
}
async function click(expression) {
  await evaluate(`(${expression}).scrollIntoView({block:'center'})`);
  await wait(150);
  const point = await evaluate(`(() => {const e=${expression}; const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  await withCdp(async (send) => {
    await send("Input.dispatchMouseEvent", { type: "mousePressed", button: "left", clickCount: 1, ...point });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "left", clickCount: 1, ...point });
  });
}
async function key(key, code, windowsVirtualKeyCode, modifiers = 0) {
  await withCdp(async (send) => {
    const params = { key, code, windowsVirtualKeyCode, modifiers };
    await send("Input.dispatchKeyEvent", { type: "keyDown", ...params });
    await send("Input.dispatchKeyEvent", { type: "keyUp", ...params });
  });
}

export async function run() {
  assert(pathUnderHome((await invoke("get_paths")).home), "必须使用隔离 HOME");
  // 仅验证本地策略 UI，不需要凭据或任何上游出网。
  const input = providerInput({ name: "限额验收超长名称 Long upstream admission policy", targetApp: "claude_code", baseUrl: "http://127.0.0.1:1", model: "ui-model" });
  input.apiKey = "";
  const upstream = await upsertUpstream(input);
  try {
    await evaluate(`(() => {localStorage.setItem('cs.language','zh-CN');localStorage.setItem('cs.layoutMode','top');})()`);
    await reloadPage();
    const nav = `document.querySelector('.v2-top-nav button[aria-label="供应商"]')`;
    await until(`Boolean(${nav})`);
    await click(nav);
    const more = `(() => { const row = Array.from(document.querySelectorAll('tr')).find(e => e.textContent.includes('限额验收超长名称')); return row ? row.querySelector('button[aria-label="更多操作"]') : null; })()`;
    const limits = `Array.from(document.querySelectorAll('.ant-dropdown-menu-item')).find(e => e.textContent.trim()==='网关限额')`;
    const openLimits = async () => {
      await until(`Boolean(${more})`);
      await click(more);
      await until(`Boolean(${limits})`);
      await click(limits);
    };
    await openLimits();
    const ready = `document.querySelector('.ant-modal input[id$="maxConcurrency"]:not(:disabled)')`;
    await until(`Boolean(${ready})`);
    const before = await invoke("get_gateway_upstream_policy", { id: upstream.id });
    assert(before.maxConcurrency === 0 && before.queueCapacity === 16, "显示默认策略");
    await click(ready);
    await key("a", "KeyA", 65, 2);
    await withCdp((send) => send("Input.insertText", { text: "2" }));
    await key("Tab", "Tab", 9);
    assert(await evaluate("Boolean(document.activeElement?.closest('.ant-modal'))"), "Tab 焦点应留在弹窗");
    const save = `Array.from(document.querySelectorAll('.ant-modal-footer button')).find(e=>e.classList.contains('ant-btn-primary'))`;
    await click(save);
    await until("!document.querySelector('.ant-modal')");
    assert((await invoke("get_gateway_upstream_policy", { id: upstream.id })).maxConcurrency === 2, "保存实际后端策略");
    await openLimits();
    await until(`Boolean(${ready})`);
    assert(await evaluate(`(${ready}).value==='2'`), "再次打开应加载已保存策略");
    const dir = path.resolve("scripts/system-test/artifacts", `limits-${process.env.AISW_CDP_PORT}-${process.pid}`);
    await fs.mkdir(dir, { recursive: true });
    const shot = await withCdp((send) => send("Page.captureScreenshot", { format: "png" }));
    const file = path.join(dir, "limits-modal.png");
    await fs.writeFile(file, Buffer.from(shot.data, "base64"));
    console.log(`[limits] ${file}`);
    await key("Escape", "Escape", 27);
    await until("!document.querySelector('.ant-modal')");
    assert(await evaluate(`document.activeElement === (${more})`), "关闭弹窗后焦点应回到入口");
  } finally {
    await invoke("delete_gateway_upstream", { id: upstream.id });
  }
}
