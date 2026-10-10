import { invoke } from "../cdp-invoke.mjs";
import {
  assert,
  createProvider,
  providerInput,
  codeBaseUrl,
  codeDiscoveryEnabled,
} from "../lib.mjs";

/** Independent current card must keep its live URL when gateway rules change. */
export const id = "SG-regress-independent-not-stolen";

export async function run() {
  const created = await createProvider(
    providerInput({
      name: "system-test-sub2api",
      targetApp: "claude_code",
      baseUrl: "https://8tou.example.test",
      model: "gpt-5.6-terra",
    })
  );
  await invoke("switch_provider", { id: created.id });
  const before = codeBaseUrl();
  assert(!before.includes(":15828"), `independent current wrote 15828: ${before}`);
  assert(!codeDiscoveryEnabled(), "discovery should stay off for an independent card");

  await invoke("list_smart_gateway_bindings");

  const after = codeBaseUrl();
  assert(after === before, `reading gateway bindings rewrote live Code env: ${before} → ${after}`);
  assert(!after.includes(":15828"), `rule edit stole the independent card: ${after}`);
}
