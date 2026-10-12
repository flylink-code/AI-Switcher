import { invoke } from "../cdp-invoke.mjs";
import { assert, providerInput, upsertUpstream } from "../lib.mjs";

/**
 * Same-name models share one catalog row owned by the highest-priority upstream.
 * Reordering upstreams changes the owner and the source order; the local gateway
 * (account login) switch round-trips through its setting.
 */
export const id = "SG-same-name-priority";

const MODEL = "system-test-shared-model";

async function sharedRow() {
  const rows = await invoke("list_gateway_catalog_entries", { target: "claude_code" });
  const matches = rows.filter((row) => row.publicId.toLowerCase().endsWith(MODEL));
  assert(matches.length === 1, `same-name model should be one row, got ${matches.length}`);
  return matches[0];
}

export async function run() {
  const first = await upsertUpstream(
    providerInput({
      name: "system-test-prio-a",
      targetApp: "claude_code",
      baseUrl: "https://prio-a.example.test",
      model: MODEL,
    })
  );
  const second = await upsertUpstream(
    providerInput({
      name: "system-test-prio-b",
      targetApp: "claude_code",
      baseUrl: "https://prio-b.example.test",
      model: MODEL,
    })
  );
  try {
    const initial = await invoke("list_gateway_upstreams");
    const indexOfFirst = initial.findIndex((row) => row.id === first.id);
    const indexOfSecond = initial.findIndex((row) => row.id === second.id);
    assert(indexOfFirst >= 0 && indexOfSecond > indexOfFirst, "new upstreams must be appended in creation order");

    const before = await sharedRow();
    assert(before.publicId === `claude.${MODEL}`, `Claude-style id should drop the provider segment: ${before.publicId}`);
    assert(before.providerName === first.name, `first upstream should own the row, got ${before.providerName}`);
    assert(
      JSON.stringify(before.sourceNames) === JSON.stringify([first.name, second.name]),
      `source order should follow priority: ${JSON.stringify(before.sourceNames)}`
    );
    const rows = await invoke("list_gateway_catalog_entries", { target: "claude_code" });
    assert(!rows.some((row) => row.publicId.toLowerCase() === "auto"), "catalog must not list auto");

    const order = initial.map((row) => row.id).filter((rowId) => rowId !== second.id);
    order.splice(order.indexOf(first.id), 0, second.id);
    const reordered = await invoke("reorder_gateway_upstreams", { orderedIds: order });
    assert(reordered.findIndex((row) => row.id === second.id) < reordered.findIndex((row) => row.id === first.id), "reorder did not take effect");

    const after = await sharedRow();
    assert(after.providerName === second.name, `second upstream should own the row after reorder, got ${after.providerName}`);
    assert(
      JSON.stringify(after.sourceNames) === JSON.stringify([second.name, first.name]),
      `source order should flip: ${JSON.stringify(after.sourceNames)}`
    );

    // Account login stays off until confirmed; API-key rows never count as "in use".
    const original = await invoke("get_local_gateway_enabled");
    assert(typeof original === "boolean", "get_local_gateway_enabled should return a boolean");
    if (!original) {
      await invoke("set_local_gateway_enabled", { enabled: true });
      assert((await invoke("get_local_gateway_enabled")) === true, "enabling local gateway did not stick");
      await invoke("set_local_gateway_enabled", { enabled: false });
      assert((await invoke("get_local_gateway_enabled")) === false, "disabling local gateway did not stick");
    }
  } finally {
    await invoke("delete_gateway_upstream", { id: first.id }).catch(() => {});
    await invoke("delete_gateway_upstream", { id: second.id }).catch(() => {});
  }
}
