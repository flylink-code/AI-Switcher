import { invoke } from "../cdp-invoke.mjs";
import { assert, pathUnderHome, testHome } from "../lib.mjs";

/** SG-A isolation: debug exe must write under AISW_TEST_HOME, not the real profile. */
export const id = "SG-isolate-home";

export async function run() {
  const home = testHome();
  const paths = await invoke("get_paths");
  assert(pathUnderHome(paths.home, home), `get_paths.home=${paths.home} expected ${home}`);
  assert(
    pathUnderHome(paths.claudeSettingsPath, home),
    `claude settings escaped isolation: ${paths.claudeSettingsPath}`
  );
  assert(
    pathUnderHome(paths.appConfigDir, home),
    `app config escaped isolation: ${paths.appConfigDir}`
  );
  for (const [name, value] of Object.entries(paths)) {
    if (typeof value === "string" && value) {
      assert(pathUnderHome(value, home), `${name} escaped isolation: ${value}`);
    }
  }
  assert(
    pathUnderHome(paths.appDbPath, home),
    `app.db escaped isolation: ${paths.appDbPath}`
  );
}
