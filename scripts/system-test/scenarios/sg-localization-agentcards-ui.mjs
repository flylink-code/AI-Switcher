import { evaluate, invoke, withCdp, reloadPage } from "../cdp-invoke.mjs";
import { assert, pathUnderHome } from "../lib.mjs";
export const id = "SG-localization-agentcards-ui";
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(expression, label = expression) {
  for (let i = 0; i < 80; i++) {
    if (await evaluate(expression)) return;
    await wait(150);
  }
  const body = await evaluate("document.body.innerText.slice(0, 1200)").catch((error) => String(error));
  throw new Error(`DOM 等待超时：${label}\n${body}`);
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
const cardByTitle = (title) =>
  `Array.from(document.querySelectorAll('.ant-card')).find(e => e.textContent.trim().startsWith('${title}') && e.querySelector('.ant-radio-group'))`;
const buttonByText = (card, text) =>
  `Array.from((${card}).querySelectorAll('button')).find(e => e.textContent.replace(/\\s/g, '') === '${text}')`;

export async function run() {
  assert(pathUnderHome((await invoke("get_paths")).home), "必须使用隔离 HOME");
  try {
    await evaluate(`(() => {
      localStorage.setItem('cs.language','zh-CN');
      localStorage.setItem('cs.layoutMode','top');
      localStorage.setItem('cs.pagePreferences',JSON.stringify({visibleAgents:['claude_code','claude_desktop']}));
    })()`);
    await reloadPage();
    // fetch 层 stub：让 Claude Code / Desktop 都显示为已安装（高级设置才会渲染），并统计上游版本请求次数。
    const patched = await evaluate(`(() => {
      window.__upstreamCalls = 0;
      const original = window.fetch;
      const info = (extra) => new Response(JSON.stringify({
        installed: true, currentVersion: '1.0.0', latestVersion: '1.0.0', updateAvailable: false,
        installCommand: '', updateCommand: '', error: null, executablePath: null, source: null,
        environment: 'native', installedButBroken: false, wslDistro: null, ...extra,
      }), { headers: { 'Content-Type': 'application/json', 'Tauri-Response': 'ok' } });
      window.fetch = function (input, init) {
        const url = typeof input === 'string' ? input : (input && input.url) || String(input);
        if (/\\/check_localization_upstream(\\?|$)/.test(url)) window.__upstreamCalls += 1;
        if (/\\/get_claude_code_version(\\?|$)/.test(url)) return Promise.resolve(info({}));
        if (/\\/get_claude_desktop_app_status(\\?|$)/.test(url)) return Promise.resolve(info({ canInstallInApp: false }));
        return original.apply(this, arguments);
      };
      return window.fetch !== original;
    })()`);
    assert(patched, "无法在 fetch 层 stub 版本检测");

    const nav = `document.querySelector('.v2-top-nav button[aria-label="Agent"]')`;
    await until(`Boolean(${nav})`, "顶部导航 Agent");
    await click(nav);

    for (const title of ["Claude Code", "Claude Desktop"]) {
      const card = cardByTitle(title);
      await until(`Boolean(${card})`, `${title} 卡片`);
      const toggle = `(${card}).querySelector('button[aria-expanded]')`;
      await until(`Boolean(${toggle})`, `${title} 高级设置开关`);
      await click(toggle);
      await until(`(${card}).textContent.includes('中文化')`, `${title} 中文化行`);
      const compact = await evaluate(`(${card}).textContent`);
      assert(compact.includes("查看详情"), `${title}：紧凑行应有「查看详情」`);
      assert(!compact.includes("宿主版本") && !compact.includes("上游版本"), `${title}：详情默认应收起`);
      // 上游查询按 queryKey 共用且有 60s 缓存：只对首张卡断言“收起 0 次、展开 ≥1 次”，第二张可能命中缓存。
      if (title === "Claude Code") {
        assert((await evaluate("window.__upstreamCalls")) === 0, `${title}：收起时不应请求上游版本`);
      }

      const details = buttonByText(card, "查看详情");
      await click(details);
      await until(`(${card}).textContent.includes('宿主版本')`, `${title} 展开详情`);
      if (title === "Claude Code") {
        await until(`window.__upstreamCalls >= 1`, `${title} 展开后请求上游版本`);
      }
      assert(await evaluate(`Boolean(${buttonByText(card, "收起详情")})`), `${title}：展开后应显示「收起详情」`);
      await click(buttonByText(card, "收起详情"));
      await until(`!(${card}).textContent.includes('宿主版本')`, `${title} 收起详情`);

    }

    // 设置页不再有汉化入口。
    const settings = `document.querySelector('.v2-top-nav button[aria-label="设置"]')`;
    await until(`Boolean(${settings})`, "顶部导航 设置");
    await click(settings);
    await until(`document.body.innerText.includes('本地代理')`, "设置页");
    const settingsText = await evaluate("document.body.innerText");
    assert(!settingsText.includes("汉化与本地化"), "设置页不应再有汉化入口");
  } finally {
    // reload 去掉 fetch 补丁。
    await reloadPage();
  }
}
