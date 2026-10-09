import { invoke } from "../cdp-invoke.mjs";
import { assert, providerInput, upsertUpstream } from "../lib.mjs";

/**
 * Global upstreams pool CRUD:
 * - Upsert upstream with ProviderInput
 * - Export and import upstreams JSON
 * - Attempt to delete an upstream referenced by direct binding fails (rejected)
 * - Delete unreferenced upstream succeeds
 *
 * Note: Verified via Tauri IPC commands, not DOM selectors.
 */
export const id = "SG-providers-pool-crud";

export async function run() {
  // 1. Upsert upstream in global pool
  const testUpstream = providerInput({
    name: "system-test-pool-crud-up",
    targetApp: "claude_code",
    baseUrl: "https://pool-crud.example.test/v1",
    model: "test-model-1",
  });
  const created = await upsertUpstream(testUpstream);
  assert(created && created.id, "upsert_gateway_upstream returned invalid provider");

  const upstreams = await invoke("list_gateway_upstreams");
  assert(
    Array.isArray(upstreams) && upstreams.some((u) => u.id === created.id),
    `created upstream ${created.id} missing from list_gateway_upstreams`
  );

  // 2. Export upstreams JSON
  const exportedJson = await invoke("export_gateway_upstreams");
  assert(typeof exportedJson === "string" && exportedJson.length > 0, "export_gateway_upstreams returned empty");
  const bundle = JSON.parse(exportedJson);
  assert(bundle.version === 1, `export version expected 1, got ${bundle.version}`);
  assert(
    Array.isArray(bundle.providers) && bundle.providers.some((p) => p.name === "system-test-pool-crud-up"),
    "exported bundle does not contain created upstream"
  );

  // 3. Import upstreams JSON
  const importResult = await invoke("import_gateway_upstreams_json", { json: exportedJson });
  assert(importResult && typeof importResult.skipped === "number", "import_gateway_upstreams_json invalid result");

  // 4. Test delete rejection when referenced by direct agent
  // Set claude_code to direct mode pointing to this upstream
  await invoke("set_agent_direct", { target: "claude_code", upstreamId: created.id });
  const mode = await invoke("get_agent_connection_mode", { target: "claude_code" });
  assert(mode === "direct", `expected mode 'direct', got '${mode}'`);

  let deleteRejected = false;
  try {
    await invoke("delete_gateway_upstream", { id: created.id });
  } catch (error) {
    deleteRejected = true;
  }
  assert(deleteRejected, `deleting upstream ${created.id} in active direct use should have been rejected`);

  // 5. Switch agent to official, then delete succeeds
  await invoke("switch_to_official", { target: "claude_code" });
  const modeAfter = await invoke("get_agent_connection_mode", { target: "claude_code" });
  assert(modeAfter === "external", `expected mode 'external' after switch_to_official, got '${modeAfter}'`);

  await invoke("delete_gateway_upstream", { id: created.id });
  const remaining = await invoke("list_gateway_upstreams");
  assert(
    !remaining.some((u) => u.id === created.id),
    `upstream ${created.id} should have been deleted after unreferencing`
  );
}
