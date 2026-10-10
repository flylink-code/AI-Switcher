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
  await evaluate(`(${expression})?.scrollIntoView({block:'nearest', behavior:'instant'})`);
  // 滚动后的 portal 可能重新定位，命中测试通过才发送指针事件。
  await new Promise((resolve) => setTimeout(resolve, 100));
  const point = await evaluate(`(() => {
    const el = (${expression});
    if (!el) throw new Error('找不到点击目标');
    const r = el.getBoundingClientRect();
    const x = r.x+r.width/2, y = r.y+r.height/2;
    const hit = document.elementFromPoint(x, y);
    if (!hit || (!el.contains(hit) && !hit.contains(el))) {
      throw new Error('点击目标被遮挡：' + JSON.stringify({target:el.outerHTML.slice(0,300),hit:hit?.outerHTML.slice(0,300),x,y}));
    }
    return {x,y};
  })()`);
  await withCdp(async (send) => {
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", ...point });
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
  // 下拉动画期间位置仍变化；等布局稳定后再发送真实指针事件。
  await new Promise((resolve) => setTimeout(resolve, 350));
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
    await evaluate("window.__aiswBeforeReload = true");
    await withCdp((send) => send("Page.reload"));
    await waitFor(() => evaluate("!window.__aiswBeforeReload").catch(() => false), "新页面加载");
    await waitForTauri();
    await waitFor(() => evaluate(`Boolean(${nav("网关")})`), "网关导航渲染");
    await click(nav("网关"));
    const routingTab = byText(".ant-segmented-item-label", "路由与规则");
    await waitFor(() => evaluate(`Boolean(${routingTab})`), "路由分区入口");
    await click(routingTab);
    await waitFor(() => bodyHas("正在编辑"), "路由档案工具栏");
    const profileSelect = `Array.from(document.querySelectorAll('.ant-card')).find(e => e.innerText.includes('正在编辑'))?.querySelector('.ant-select')`;
    await click(profileSelect);
    await option("L2 UI 隔离档案");
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

    const beforeDrift = await invoke("get_agent_config_drift", { target: "claude_code" });
    assert(beforeDrift.status === "in_sync", "写入后应有可比对快照");
    const settingsPath = path.join(paths.home, ".claude", "settings.json");
    assert(pathUnderHome(settingsPath), "漂移操作必须位于隔离 HOME");
    const settings = JSON.parse(await fs.readFile(settingsPath, "utf8"));
    settings.env.ANTHROPIC_MODEL = "l2-external-drift";
    await fs.writeFile(settingsPath, JSON.stringify(settings));
    const drift = await invoke("get_agent_config_drift", { target: "claude_code" });
    assert(drift.status === "drifted", "应识别外部修改");
    assert(!JSON.stringify(drift).includes("l2-external-drift"), "预览不得返回字段原值");
    let rejected = false;
    try {
      await invoke("reapply_agent_config", { target: "claude_code", revision: beforeDrift.revision });
    } catch { rejected = true; }
    assert(rejected, "过期预览必须拒绝重新应用");
    const applied = await invoke("reapply_agent_config", { target: "claude_code", revision: drift.revision });
    assert(applied.status === "in_sync", "显式重新应用后恢复一致");
    assert((await invoke("get_agent_connection_mode", { target: "claude_code" })) === "gateway", "重新应用不能切换连接方式");

    await invoke("bind_smart_gateway", { target: "codex" });
    const codexBefore = await invoke("get_agent_config_drift", { target: "codex" });
    assert(codexBefore.status === "in_sync", "Codex 写出后应建立基线");
    const codexConfigPath = path.join(paths.home, ".codex", "config.toml");
    assert(pathUnderHome(codexConfigPath), "Codex 漂移操作必须位于隔离 HOME");
    const codexConfig = await fs.readFile(codexConfigPath, "utf8");
    assert(/^model\s*=.*$/m.test(codexConfig), "Codex 配置应包含托管模型");
    await fs.writeFile(codexConfigPath, codexConfig.replace(/^model\s*=.*$/m, 'model = "l2-codex-drift"'));
    const codexDrift = await invoke("get_agent_config_drift", { target: "codex" });
    assert(codexDrift.status === "drifted", "应识别 Codex 外部修改");
    assert(!JSON.stringify(codexDrift).includes("l2-codex-drift"), "Codex 预览不返回原值");
    const codexApplied = await invoke("reapply_agent_config", { target: "codex", revision: codexDrift.revision });
    assert(codexApplied.status === "in_sync", "Codex 重新应用后恢复一致");
    assert(await invoke("get_agent_connection_mode", { target: "codex" }) === "gateway", "Codex 重新应用不切换连接方式");
  } catch (error) {
    console.log("[ui] body", await evaluate("document.body.innerText"));
    console.log("[ui] profiles", JSON.stringify(await invoke("list_gateway_profiles")));
    await screenshot("failure").catch(() => {});
    throw error;
  } finally {
    await invoke("stop_smart_gateway");
    await invoke("switch_to_official", { target: "claude_code" });
    await invoke("switch_to_official", { target: "codex" });
    await invoke("delete_gateway_profile", { id: profile.id });
    await invoke("delete_gateway_upstream", { id: upstream.id });
  }
}
