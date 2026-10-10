import fs from "node:fs/promises";
import path from "node:path";
import { evaluate, withCdp, reloadPage, invoke } from "../cdp-invoke.mjs";
import { assert, pathUnderHome } from "../lib.mjs";

export const id = "SG-ui-layout-matrix";

async function waitFor(expression) {
  for (let i = 0; i < 80; i++) {
    if (await evaluate(expression)) return;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`DOM 等待超时：${expression}`);
}

export async function run() {
  assert(pathUnderHome((await invoke("get_paths")).home), "只允许隔离 HOME");
  const dir = path.resolve("scripts/system-test/artifacts", `matrix-${process.env.AISW_CDP_PORT}-${process.pid}`);
  await fs.mkdir(dir, { recursive: true });
  for (const layout of ["top", "sidebar"]) {
    for (const theme of ["light", "dark"]) {
      for (const language of ["zh-CN", "en-US"]) {
        await evaluate(`(() => {
          localStorage.setItem('cs.layoutMode',${JSON.stringify(layout)});
          localStorage.setItem('cs.theme',${JSON.stringify(theme)});
          localStorage.setItem('cs.language',${JSON.stringify(language)});
          localStorage.setItem('cs.sideNavCollapsed','false');
        })()`);
        await reloadPage();
        const labels = language === "zh-CN" ? ["概览", "供应商", "网关"] : ["Dashboard", "Providers", "Gateway"];
        for (let index = 0; index < labels.length; index++) {
          const label = labels[index];
          const expr = `Array.from(document.querySelectorAll('.v2-top-nav button,.side-nav-item')).find(e => e.getAttribute('aria-label') === ${JSON.stringify(label)} || e.textContent.trim() === ${JSON.stringify(label)})`;
          await waitFor(`Boolean(${expr})`);
          const point = await evaluate(`(() => {const e=${expr}; e.scrollIntoView({block:'center'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
          await withCdp(async (send) => {
            await send("Input.dispatchMouseEvent", { type: "mousePressed", button: "left", clickCount: 1, ...point });
            await send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "left", clickCount: 1, ...point });
          });
          await new Promise((resolve) => setTimeout(resolve, 900));
          assert(await evaluate("document.documentElement.scrollWidth <= window.innerWidth + 2"), `${layout}/${theme}/${language}/${label} 页面横向溢出`);
          assert(!await evaluate("/configDrift\\.|topology\\./.test(document.body.innerText)"), "不能显示未翻译 key");
          const shot = await withCdp((send) => send("Page.captureScreenshot", { format: "png" }));
          const file = path.join(dir, `${layout}-${theme}-${language}-${index}.png`);
          await fs.writeFile(file, Buffer.from(shot.data, "base64"));
          console.log(`[matrix] ${file}`);
        }
      }
    }
  }
  const overview = `Array.from(document.querySelectorAll('.v2-top-nav button,.side-nav-item')).find(e => e.getAttribute('aria-label') === 'Dashboard' || e.textContent.trim() === 'Dashboard')`;
  const point = await evaluate(`(() => {const e=${overview}; const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  await withCdp(async (send) => {
    await send("Input.dispatchMouseEvent", { type: "mousePressed", button: "left", clickCount: 1, ...point });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "left", clickCount: 1, ...point });
  });
  await waitFor("Boolean(document.querySelector('.gateway-topology-result'))");
  await withCdp(async (send) => {
    await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] });
    try {
      assert(await evaluate("matchMedia('(prefers-reduced-motion: reduce)').matches"), "reduced-motion 偏好应生效");
      assert(await evaluate("Array.from(document.querySelectorAll('.gateway-topology-result')).every(e => getComputedStyle(e).animationName === 'none')"), "reduced-motion 必须关闭拓扑动画");
    } finally {
      await send("Emulation.setEmulatedMedia", { features: [] });
    }
  });
}
