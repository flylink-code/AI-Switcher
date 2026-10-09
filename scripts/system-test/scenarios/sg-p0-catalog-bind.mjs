import fs from "node:fs";
import { invoke } from "../cdp-invoke.mjs";
import { assert, createProvider, isolatedOpenCodePath, providerInput, upsertUpstream } from "../lib.mjs";

/** Catalog-style bind writes Auto as only managed entry; legacy direct kept in DB but not in live config. */
export const id = "SG-p0-catalog-bind-appends-auto";

export async function run() {
  await createProvider(
    providerInput({
      name: "system-test-oc-direct",
      targetApp: "opencode",
      baseUrl: "https://direct.example.test/v1",
      model: "gpt-5.6-terra",
      protocolType: "openai_chat",
    })
  );

  const pools = await invoke("list_gateway_upstreams");
  if (!Array.isArray(pools) || pools.length === 0) {
    await upsertUpstream(
      providerInput({
        name: "system-test-oc-pool",
        targetApp: "claude_code",
        baseUrl: "https://pool.example.test",
        model: "claude-sonnet-custom",
      })
    );
  }

  await invoke("bind_smart_gateway", { target: "opencode" });
  const livePath = isolatedOpenCodePath();
  const text = fs.existsSync(livePath) ? fs.readFileSync(livePath, "utf8") : "";

  // Auto is now the only entry written to live config; direct card is NOT in live config
  assert(
    !text.includes("direct.example.test") && !text.includes("system-test-oc-direct"),
    `legacy direct OpenCode provider should not be in live config ${livePath}: ${text.slice(0, 400)}`
  );
  assert(
    text.includes("ai-switcher") || text.includes("15828") || /16\d{3}/.test(text),
    `binding did not write Auto entry in ${livePath}: ${text.slice(0, 400)}`
  );

  // In database, legacy direct provider is preserved alongside smart_gateway
  const providers = await invoke("list_providers", { target: "opencode" });
  assert(
    providers.some((row) => row.providerKind === "smart_gateway"),
    "OpenCode Auto card missing after bind"
  );
  assert(
    providers.some((row) => row.name === "system-test-oc-direct"),
    "independent OpenCode card was removed"
  );
}
