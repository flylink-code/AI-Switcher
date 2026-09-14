import { invoke } from "../cdp-invoke.mjs";
import { assert } from "../lib.mjs";

/** P0 local-proxy hop: exclusive non-gateway cards should request a proxy listen port. */
export const id = "SG-p0-agent-proxy-hop";

export async function run() {
  const current = await invoke("get_current_provider", { target: "claude_code" });
  if (!current || current.providerKind === "smart_gateway") {
    console.log("skip: no independent Claude Code current card (bind scenario already ran)");
    return;
  }
  const status = await invoke("get_proxy_status", { target: "claude_code" });
  const port = status?.port;
  const running = status?.running;
  if (running === false) {
    console.log("skip: local proxy not running (no live traffic to log hop=agent_proxy)");
    return;
  }
  if (port) {
    assert(Number(port) !== 15828, `independent card still on gateway port ${port}`);
  }
  const logs = await invoke("list_proxy_request_logs_cmd", {
    input: { hours: 1, targetApp: "claude_code", page: 0, pageSize: 20 },
  });
  const hops = (logs?.data || []).map((row) => row.hop).filter(Boolean);
  if (hops.length === 0) {
    console.log("ok: no live requests yet; hop=agent_proxy is covered by L1 + optional Claude Code");
    return;
  }
  assert(
    hops.includes("agent_proxy") || hops.every((hop) => hop !== "smart_gateway"),
    `expected agent_proxy (or no smart_gateway) hops, got ${JSON.stringify(hops)}`
  );
}
