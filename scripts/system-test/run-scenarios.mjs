#!/usr/bin/env node
import { waitForTauri } from "./cdp-invoke.mjs";

const SCENARIOS = [
  "./scenarios/sg-isolate-home.mjs",
  "./scenarios/sg-regress-no-autobind.mjs",
  "./scenarios/sg-regress-independent-not-stolen.mjs",
  "./scenarios/sg-p0-simulate.mjs",
  "./scenarios/sg-p0-catalog-bind.mjs",
  "./scenarios/sg-regress-auto-current.mjs",
  "./scenarios/sg-p0-agent-proxy-hop.mjs",
];

const filter = (process.env.AISW_SCENARIO || "*").trim();

function matchFilter(id) {
  if (!filter || filter === "*") {
    return true;
  }
  return id.includes(filter) || filter.split(",").some((part) => id.includes(part.trim()));
}

const started = Date.now();
const results = [];

const ready = await waitForTauri();
console.log(`[system-test] Tauri IPC ready origin=${ready.origin} href=${ready.href}`);

for (const specifier of SCENARIOS) {
  const mod = await import(specifier);
  const id = mod.id || specifier;
  if (!matchFilter(id)) {
    continue;
  }
  const t0 = Date.now();
  try {
    await mod.run();
    const ms = Date.now() - t0;
    console.log(`ok   ${id}  (${ms}ms)`);
    results.push({ id, ok: true, ms });
  } catch (error) {
    const ms = Date.now() - t0;
    const message = error instanceof Error ? error.message : String(error);
    console.error(`FAIL ${id}  (${ms}ms)`);
    console.error(`     ${message}`);
    results.push({ id, ok: false, ms, message });
    if (process.env.AISW_KEEP_GOING !== "1") {
      break;
    }
  }
}

const failed = results.filter((row) => !row.ok);
console.log(
  `\n${results.length - failed.length}/${results.length} passed in ${Date.now() - started}ms`
);
if (failed.length > 0) {
  process.exit(1);
}
