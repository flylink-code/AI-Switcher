import { invoke } from "../cdp-invoke.mjs";
import { assert, codeBaseUrl, providerInput, upsertUpstream, SHARED_PROFILE_ID } from "../lib.mjs";

/**
 * Agent Connection Card state machine:
 * - IPC verification of set_agent_direct / bind_smart_gateway / switch_to_official
 * - Unbound (official) rejects set_gateway_binding_profile
 * - Direct mode rejects set_gateway_binding_profile and does NOT write 15828
 * - Direct upstream deletion rejected while in use
 * - Gateway mode connects to 15828 and allows profile selection
 *
 * Note: Verified via Tauri IPC commands, not DOM selectors.
 */
export const id = "SG-agent-connection-card";

export async function run() {
  // 1. Ensure initial state is official
  await invoke("switch_to_official", { target: "claude_code" });
  const initialMode = await invoke("get_agent_connection_mode", { target: "claude_code" });
  assert(initialMode === "external", `expected initial mode 'external', got '${initialMode}'`);

  const initialUrl = codeBaseUrl();
  assert(!initialUrl.includes("15828"), `official mode should not have 15828 in base URL: ${initialUrl}`);

  // Without a binding, set_gateway_binding_profile must be rejected
  let unboundProfileRejected = false;
  try {
    await invoke("set_gateway_binding_profile", {
      target: "claude_code",
      profileId: SHARED_PROFILE_ID,
    });
  } catch (error) {
    unboundProfileRejected = true;
  }
  assert(unboundProfileRejected, "set_gateway_binding_profile on unbound target should be rejected");

  // 2. Create upstream and switch to direct mode
  const directUpstream = await upsertUpstream(
    providerInput({
      name: "system-test-direct-card-up",
      targetApp: "claude_code",
      baseUrl: "https://direct-card.example.test/v1",
      model: "claude-3-7-sonnet-20250219",
    })
  );
  assert(directUpstream && directUpstream.id, "failed to create upstream for direct test");

  await invoke("set_agent_direct", {
    target: "claude_code",
    upstreamId: directUpstream.id,
  });

  const directMode = await invoke("get_agent_connection_mode", { target: "claude_code" });
  assert(directMode === "direct", `expected mode 'direct', got '${directMode}'`);

  // Direct mode must NOT write 15828
  const directUrl = codeBaseUrl();
  assert(
    !directUrl.includes("15828"),
    `direct mode must not contain 15828: ${directUrl}`
  );
  assert(
    directUrl.includes("direct-card.example.test"),
    `direct mode should write upstream base URL: ${directUrl}`
  );

  // In direct mode, set_gateway_binding_profile must be rejected
  let directProfileRejected = false;
  try {
    await invoke("set_gateway_binding_profile", {
      target: "claude_code",
      profileId: SHARED_PROFILE_ID,
    });
  } catch (error) {
    directProfileRejected = true;
  }
  assert(directProfileRejected, "set_gateway_binding_profile in direct mode should be rejected");

  // Direct upstream cannot be deleted while referenced
  let directDeleteRejected = false;
  try {
    await invoke("delete_gateway_upstream", { id: directUpstream.id });
  } catch (error) {
    directDeleteRejected = true;
  }
  assert(directDeleteRejected, "deleting upstream referenced by direct agent should be rejected");

  // 3. Switch to gateway mode
  await invoke("bind_smart_gateway", { target: "claude_code" });
  const gatewayMode = await invoke("get_agent_connection_mode", { target: "claude_code" });
  assert(gatewayMode === "gateway", `expected mode 'gateway', got '${gatewayMode}'`);

  const gatewayUrl = codeBaseUrl();
  assert(
    gatewayUrl.includes("15828") || /16\d{3}/.test(gatewayUrl),
    `gateway mode must contain 15828: ${gatewayUrl}`
  );

  // In gateway mode, set_gateway_binding_profile succeeds
  const binding = await invoke("set_gateway_binding_profile", {
    target: "claude_code",
    profileId: SHARED_PROFILE_ID,
  });
  assert(binding && binding.profileId === SHARED_PROFILE_ID, "set_gateway_binding_profile should succeed in gateway mode");

  // 4. Restore official
  await invoke("switch_to_official", { target: "claude_code" });
  const officialMode = await invoke("get_agent_connection_mode", { target: "claude_code" });
  assert(officialMode === "external", `expected mode 'external', got '${officialMode}'`);

  const officialUrl = codeBaseUrl();
  assert(!officialUrl.includes("15828"), `restored official should not contain 15828: ${officialUrl}`);

  // Cleanup upstream now that it is no longer referenced
  await invoke("delete_gateway_upstream", { id: directUpstream.id });
}
