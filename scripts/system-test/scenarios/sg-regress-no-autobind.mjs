import { invoke } from "../cdp-invoke.mjs";
import { assert, expectInvokeError, SHARED_PROFILE_ID } from "../lib.mjs";

/** SG-regress-no-autobind: picking a profile must not create gateway_bindings. */
export const id = "SG-regress-no-autobind";

export async function run() {
  const before = await invoke("list_smart_gateway_bindings");
  assert(Array.isArray(before), "list_smart_gateway_bindings should return an array");
  const countBefore = before.length;

  await expectInvokeError(
    "set_gateway_binding_profile",
    { target: "opencode", profileId: SHARED_PROFILE_ID },
    "请先绑定智能网关"
  );

  const after = await invoke("list_smart_gateway_bindings");
  assert(after.length === countBefore, "unbound profile pick created a binding row");
  assert(
    !after.some((row) => row.targetApp === "opencode" || row.target === "opencode"),
    "opencode binding appeared without bind_smart_gateway"
  );
}
