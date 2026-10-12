import { invoke } from "../cdp-invoke.mjs";
import {
  assert,
  emptyMapping,
  isolatedSettingsPath,
  providerInput,
  readJson,
  upsertUpstream,
} from "../lib.mjs";

/**
 * Agent-level direct model mapping:
 * - set_agent_direct_model_mapping while direct rewrites ANTHROPIC_DEFAULT_*
 * - empty roles fall back to the upstream's stored mapping, then the default model
 * - clearing the mapping restores the upstream/default values
 * - non Claude agents are rejected
 */
export const id = "SG-agent-direct-mapping";

function liveEnv() {
  return readJson(isolatedSettingsPath())?.env || {};
}

export async function run() {
  await invoke("switch_to_official", { target: "claude_code" });

  const upstream = await upsertUpstream(
    providerInput({
      name: "system-test-direct-mapping-up",
      targetApp: "claude_code",
      baseUrl: "https://direct-mapping.example.test/v1",
      model: "up-default-model",
      modelMapping: { ...emptyMapping(), opus: "up-opus-model" },
    })
  );
  assert(upstream && upstream.id, "failed to create upstream for mapping test");

  try {
    await invoke("set_agent_direct", { target: "claude_code", upstreamId: upstream.id });
    let env = liveEnv();
    assert(env.ANTHROPIC_DEFAULT_SONNET_MODEL === "up-default-model", `sonnet follows default: ${env.ANTHROPIC_DEFAULT_SONNET_MODEL}`);
    assert(env.ANTHROPIC_DEFAULT_OPUS_MODEL === "up-opus-model", `opus follows upstream mapping: ${env.ANTHROPIC_DEFAULT_OPUS_MODEL}`);

    const saved = await invoke("set_agent_direct_model_mapping", {
      target: "claude_code",
      mapping: { ...emptyMapping(), sonnet: " agent-sonnet-model " },
    });
    assert(saved.sonnet === "agent-sonnet-model", `mapping must be trimmed: ${JSON.stringify(saved)}`);
    env = liveEnv();
    assert(env.ANTHROPIC_DEFAULT_SONNET_MODEL === "agent-sonnet-model", `agent mapping wins: ${env.ANTHROPIC_DEFAULT_SONNET_MODEL}`);
    assert(env.ANTHROPIC_DEFAULT_OPUS_MODEL === "up-opus-model", `empty agent role keeps upstream mapping: ${env.ANTHROPIC_DEFAULT_OPUS_MODEL}`);

    const read = await invoke("get_agent_direct_model_mapping", { target: "claude_code" });
    assert(read.sonnet === "agent-sonnet-model", "get must return the saved agent mapping");

    await invoke("set_agent_direct_model_mapping", { target: "claude_code", mapping: emptyMapping() });
    env = liveEnv();
    assert(env.ANTHROPIC_DEFAULT_SONNET_MODEL === "up-default-model", `cleared mapping restores default: ${env.ANTHROPIC_DEFAULT_SONNET_MODEL}`);

    let rejected = false;
    try {
      await invoke("set_agent_direct_model_mapping", { target: "codex", mapping: emptyMapping() });
    } catch {
      rejected = true;
    }
    assert(rejected, "codex must not accept a direct role mapping");
  } finally {
    await invoke("set_agent_direct_model_mapping", { target: "claude_code", mapping: emptyMapping() }).catch(() => undefined);
    await invoke("switch_to_official", { target: "claude_code" });
    await invoke("delete_gateway_upstream", { id: upstream.id });
  }
}
