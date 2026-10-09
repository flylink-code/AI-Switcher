import fs from "node:fs/promises";
import path from "node:path";
import { evaluate, invoke, withCdp, waitForTauri } from "../cdp-invoke.mjs";
import { assert, pathUnderHome, providerInput, upsertUpstream, SHARED_PROFILE_ID } from "../lib.mjs";

export const id = "SG-gateway-ui-reliability";

async function waitFor(check, label, timeout = 15_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`等待超时：${label}`);
}

async function click(expression) {
  const point = await evaluate(`(() => {
    const el = (${expression});
    if (!el) throw new Error('找不到点击目标');
    el.scrollIntoView({block:'center'});
    const r = el.getBoundingClientRect();
    return {x:r.x+r.width/2,y:r.y+r.height/2};
  })()`);
  await withCdp(async (send) => {
    await send("Input.dispatchMouseEvent", { type: "mousePressed", button: "left", clickCount: 1, ...point });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", button: "left", clickCount: 1, ...point });
  });
}

const byText = (selector, text) => `Array.from(document.querySelectorAll(${JSON.stringify(selector)})).find(e => e.textContent.trim() === ${JSON.stringify(text)})`;
const nav = (name) => `document.querySelector('.v2-top-nav button[aria-label="${name}"]')`;
const bodyHas = (text) => evaluate(`document.body.innerText.includes(${JSON.stringify(text)})`);

async function option(text) {
  const expression = byText(".ant-select-dropdown:not(.ant-select-dropdown-hidden) .ant-select-item-option-content", text);
  await waitFor(() => evaluate(`Boolean(${expression})`), `下拉选项 ${text}`);
  await click(expression);
}

async function screenshot(name) {
  const dir = path.resolve("scripts/system-test/artifacts", `ui-${process.env.AISW_CDP_PORT}-${process.pid}`);
  await fs.mkdir(dir, { recursive: true });
  const result = await withCdp((send) => send("Page.captureScreenshot", { format: "png" }));
  const file = path.join(dir, `${name}.png`);
  await fs.writeFile(file, Buffer.from(result.data, "base64"));
  console.log(`[ui] screenshot ${file}`);
}

export async function run() {
  const paths = await invoke("get_paths");
  assert(pathUnderHome(paths.home), "DOM 测试只能操作隔离 HOME");
  await invoke("set_smart_gateway_health_probe_secs", { secs: 0 });
  await invoke("switch_to_official", { target: "claude_code" });
  await invoke("switch_to_official", { target: "codex" });
  const profiles = await invoke("list_gateway_profiles");
  const shared = profiles.find((row) => row.id === SHARED_PROFILE_ID);
  const profile = await invoke("create_gateway_profile", { name: "L2 UI 隔离档案", cloneFrom: SHARED_PROFILE_ID });
  const upstream = await upsertUpstream(providerInput({
    name: "L2 UI 上游", targetApp: "claude_code", baseUrl: "http://127.0.0.1:1", model: "ui-primary",
  }));
  try {
    await invoke("set_agent_direct", { target: "claude_code", upstreamId: upstream.id });
    // 仅在隔离 WebView 中固定测试语言与布局；页面操作仍走真实 DOM 事件。
    await evaluate(`(() => {
      localStorage.setItem('cs.language','zh-CN');
      localStorage.setItem('cs.layoutMode','top');
      localStorage.setItem('cs.pagePreferences',JSON.stringify({visibleAgents:['claude_code','codex'],gatewayTab:'smart',gatewaySection:'routing',gatewayProfileId:${JSON.stringify(profile.id)}}));
    })()`);
    await withCdp((send) => send("Page.reload"));
    await waitForTauri();
    await waitFor(() => evaluate(`Boolean(${nav("网关")})`), "网关导航渲染");
    await click(nav("网关"));
    await waitFor(() => bodyHas("L2 UI 隔离档案"), "正在编辑档案");
    const fallbackSelect = `Array.from(document.querySelectorAll('.ant-card')).find(e => e.innerText.includes('正在编辑'))?.querySelectorAll('.ant-select')[1]`;
    await click(fallbackSelect);
    await option("备用链 (model_chain)");
    await waitFor(async () => (await invoke("list_gateway_profiles")).find((row) => row.id === profile.id)?.fallbackMode === "model_chain", "按 ID 保存备用方式");
    assert((await invoke("list_gateway_profiles")).find((row) => row.id === SHARED_PROFILE_ID)?.fallbackMode === shared.fallbackMode, "编辑其它档案不能改默认档案");
    assert(await invoke("get_agent_connection_mode", { target: "claude_code" }) === "direct", "编辑档案不能抢占 direct");
    assert(await invoke("get_agent_connection_mode", { target: "codex" }) === "external", "编辑档案不能隐式绑定 Codex");
    await screenshot("routing-profile");

    await invoke("bind_smart_gateway", { target: "claude_code" });
    await invoke("stop_smart_gateway");
    const status = await invoke("get_smart_gateway_status");
    assert(!status.running && status.port !== 15828, "应使用停止状态的隔离网关端口");
    await click(nav("供应商"));
    await waitFor(() => bodyHas(`网关未启动 (:${status.port})`), "连接卡显示真实端口和停止状态");
    await screenshot("gateway-stopped");
    await invoke("start_smart_gateway");
    await waitFor(() => bodyHas(`已连接网关 (:${status.port})`), "连接卡刷新运行状态", 20_000);
    await screenshot("gateway-running");
  } catch (error) {
    await screenshot("failure").catch(() => {});
    throw error;
  } finally {
    await invoke("stop_smart_gateway");
    await invoke("switch_to_official", { target: "claude_code" });
    await invoke("delete_gateway_profile", { id: profile.id });
    await invoke("delete_gateway_upstream", { id: upstream.id });
  }
}
