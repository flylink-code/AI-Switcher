import { evaluate, invoke, withCdp, reloadPage } from "../cdp-invoke.mjs";
import { assert, pathUnderHome } from "../lib.mjs";

export const id = "SG-codex-cli-optional-ui";
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

// Codex 卡片：以标题开头匹配（展开「高级设置」后正文会出现 Claude 字样，不能再按“不含 Claude”过滤）。
const codexCard = `Array.from(document.querySelectorAll('.ant-card')).find(e => e.textContent.trim().startsWith('Codex') && e.querySelector('.ant-radio-group'))`;

export async function run() {
  assert(pathUnderHome((await invoke("get_paths")).home), "必须使用隔离 HOME");
  try {
    await evaluate(`(() => {
      localStorage.setItem('cs.language','zh-CN');
      localStorage.setItem('cs.layoutMode','top');
      localStorage.setItem('cs.pagePreferences',JSON.stringify({visibleAgents:['claude_code','codex']}));
    })()`);
    await reloadPage();
    // 页面级打补丁：__TAURI_INTERNALS__.invoke / ipc 均为只读不可配置，改在 IPC 的 fetch 层拦截，
    // 只让 get_codex_cli_version 返回“未安装”，其余命令透传；reload 后补丁消失。
    const patched = await evaluate(`(() => {
      const original = window.fetch;
      window.fetch = function (input, init) {
        const url = typeof input === 'string' ? input : (input && input.url) || String(input);
        if (/\\/get_codex_cli_version(\\?|$)/.test(url)) {
          return Promise.resolve(new Response(JSON.stringify({
            installed: false, currentVersion: null, latestVersion: null, updateAvailable: false,
            installCommand: '', updateCommand: '', error: null, executablePath: null, source: null,
            environment: 'native', installedButBroken: false,
          }), { headers: { 'Content-Type': 'application/json', 'Tauri-Response': 'ok' } }));
        }
        return original.apply(this, arguments);
      };
      return window.fetch !== original;
    })()`);
    assert(patched, "无法在 fetch 层 stub get_codex_cli_version");

    const nav = `document.querySelector('.v2-top-nav button[aria-label="Agent"]')`;
    await until(`Boolean(${nav})`, "顶部导航 Agent");
    await click(nav);
    await until(`Boolean(${codexCard})`, "Codex 卡片");

    // 展开高级设置并点“检测”，强制按补丁后的结果重新拉取 CLI 状态。
    const toggle = `(${codexCard}).querySelector('button[aria-expanded]')`;
    await until(`Boolean(${toggle})`, "Codex 高级设置开关（无 CLI 时也应存在）");
    await click(toggle);
    const detect = `Array.from((${codexCard}).querySelectorAll('button')).find(e => e.textContent.replace(/\\s/g, '') === '检测')`;
    await until(`Boolean(${detect})`, "检测按钮");
    await click(detect);
    await until(`(${codexCard}).textContent.includes('CLI 未安装（可选）')`, "CLI 未安装（可选）提示");

    const text = await evaluate(`(${codexCard}).textContent`);
    assert(text.includes("智能网关") && text.includes("直连") && text.includes("官方"), "无 CLI 时连接模式仍可用");
    const hasInstallPrimary = await evaluate(`Array.from((${codexCard}).querySelectorAll('button.ant-btn-primary')).some(e => e.textContent.replace(/\\s/g, '') === '安装')`);
    assert(!hasInstallPrimary, "Codex 卡片不应出现主「安装」按钮");
    assert(text.includes("安装 CLI"), "维护行应提供「安装 CLI」");
    assert(!(await evaluate(`Array.from((${codexCard}).querySelectorAll('button')).some(e => e.textContent.replace(/\\s/g, '') === '升级')`)), "无 CLI 时不应显示「升级」");
  } finally {
    // reload 去掉 invoke 补丁。
    await reloadPage();
  }
}
