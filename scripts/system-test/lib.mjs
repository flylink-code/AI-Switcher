import fs from "node:fs";
import path from "node:path";
import { invoke } from "./cdp-invoke.mjs";

export const SHARED_PROFILE_ID = "gprof_shared";

export function testHome() {
  const home = process.env.AISW_TEST_HOME;
  if (!home) {
    throw new Error("AISW_TEST_HOME is required");
  }
  return home;
}

export function normalizePath(value) {
  return String(value || "")
    .replaceAll("/", "\\")
    .replace(/\\+$/g, "")
    .toLowerCase();
}

export function pathUnderHome(value, home = testHome()) {
  const full = normalizePath(value);
  const root = normalizePath(home);
  return full === root || full.startsWith(`${root}\\`);
}

export function emptyMapping() {
  return { sonnet: "", opus: "", haiku: "", fable: "", subagent: "" };
}

export function providerInput({
  id,
  name,
  targetApp,
  baseUrl,
  model,
  protocolType = "anthropic",
}) {
  const input = {
    name,
    baseUrl,
    apiKey: "sk-system-test",
    model,
    modelMapping: emptyMapping(),
    protocolType,
    providerKind: "standard",
    targetApp,
    notes: "system-test",
    failoverGroup: 0,
    failoverModels: [],
    hiddenModels: [],
  };
  if (id) {
    input.id = id;
  }
  return input;
}

export function readJson(filePath) {
  if (!fs.existsSync(filePath)) {
    return null;
  }
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

export function isolatedSettingsPath() {
  return path.join(testHome(), ".claude", "settings.json");
}

export function isolatedOpenCodePath() {
  return (
    process.env.OPENCODE_CONFIG ||
    path.join(testHome(), ".config", "opencode", "opencode.json")
  );
}

export function codeBaseUrl() {
  const settings = readJson(isolatedSettingsPath()) || {};
  return settings.env?.ANTHROPIC_BASE_URL || "";
}

export function codeDiscoveryEnabled() {
  const settings = readJson(isolatedSettingsPath()) || {};
  return settings.env?.CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY === "1";
}

export function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

export async function createProvider(input) {
  return invoke("create_provider", { input });
}

export async function upsertUpstream(input) {
  return invoke("upsert_gateway_upstream", { input });
}

export async function expectInvokeError(command, args, match) {
  try {
    await invoke(command, args);
  } catch (error) {
    const text = error instanceof Error ? error.message : String(error);
    if (match && !text.includes(match)) {
      throw new Error(`expected error containing ${JSON.stringify(match)}, got: ${text}`);
    }
    return text;
  }
  throw new Error(`${command} should have failed`);
}
