import { invoke } from "../cdp-invoke.mjs";
import {
  assert,
  codeBaseUrl,
  codeDiscoveryEnabled,
  createProvider,
  providerInput,
  upsertUpstream,
} from "../lib.mjs";

/** Bind + Auto current writes 15828 (or the isolated gateway port); leaving Auto clears it. */
export const id = "SG-regress-auto-current-writes-gateway";

export async function run() {
  const independent = await createProvider(
    providerInput({
      name: "system-test-code-direct",
      targetApp: "claude_code",
      baseUrl: "https://direct.example.test",
      model: "gpt-5.6-terra",
    })
  );
  await invoke("switch_provider", { id: independent.id });

  await upsertUpstream(
    providerInput({
      name: "system-test-pool",
      targetApp: "claude_code",
      baseUrl: "https://pool.example.test",
      model: "claude-sonnet-custom",
    })
  );

  const status = await invoke("get_smart_gateway_status");
  const gatewayPort = status.port || 15828;
  const auto = await invoke("bind_smart_gateway", { target: "claude_code" });
  assert(auto.providerKind === "smart_gateway", "bind should return the Auto card");
  assert(
    codeBaseUrl().includes(`:${gatewayPort}`),
    `Auto current should write :${gatewayPort}, got ${codeBaseUrl()}`
  );
  assert(codeDiscoveryEnabled(), "Auto current should enable gateway model discovery");

  await invoke("switch_provider", { id: independent.id });
  assert(
    !codeBaseUrl().includes(`:${gatewayPort}`) && !codeBaseUrl().includes(":15828"),
    `switching off Auto left the gateway URL: ${codeBaseUrl()}`
  );
  assert(!codeDiscoveryEnabled(), "leaving Auto should clear gateway discovery");
}
