import { invoke } from "../cdp-invoke.mjs";
import { assert, providerInput, SHARED_PROFILE_ID, upsertUpstream } from "../lib.mjs";

/** claude.auto with a non-empty mode table must not fall through to gpt-6-astra. */
export const id = "SG-p0-simulate-default-mode";

export async function run() {
  const upstream = await upsertUpstream(
    providerInput({
      name: "system-test-sim-pool",
      targetApp: "claude_code",
      baseUrl: "https://sim.example.test",
      model: "claude-sonnet-custom",
    })
  );
  await invoke("update_route_mode", {
    id: "default",
    patch: { enabled: true, model: "claude-sonnet-custom" },
    profileId: SHARED_PROFILE_ID,
  });

  const result = await invoke("simulate_gateway_route", {
    input: {
      requestedModel: "claude.auto",
      tokenCount: 32,
      hasWebSearch: false,
      hasVision: false,
      hasThinking: false,
      isSubagent: false,
      isImageGen: false,
      toolNames: [],
      path: "/v1/messages",
      target: "claude_code",
      profileId: SHARED_PROFILE_ID,
    },
  });

  const decision = result.decision;
  assert(decision, `simulate returned no decision: ${JSON.stringify(result)}`);
  assert(
    decision.normalizedModel !== "gpt-6-astra",
    "non-empty modes fell through to gpt-6-astra"
  );
  const hitDefault =
    decision.modeId === "default" ||
    String(decision.normalizedModel || "").includes("claude-sonnet-custom") ||
    String(result.upstreamModel || "").includes("claude-sonnet-custom");
  assert(
    hitDefault,
    `expected default-mode routing, got ${JSON.stringify({
      modeId: decision.modeId,
      model: decision.normalizedModel,
      upstream: result.upstreamModel,
      upstreamId: upstream.id,
    })}`
  );
}
