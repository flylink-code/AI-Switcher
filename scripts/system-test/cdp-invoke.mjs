#!/usr/bin/env node
/**
 * Invoke a Tauri command in the running debug window via CDP :9222
 * (`__TAURI_INTERNALS__.invoke`), the same path used for local L2 checks.
 *
 *   node scripts/system-test/cdp-invoke.mjs get_paths
 *   node scripts/system-test/cdp-invoke.mjs list_providers claude_code
 *   node scripts/system-test/cdp-invoke.mjs create_provider '{"name":"..."}'
 */

const CDP_PORT = Number(process.env.AISW_CDP_PORT || "9222");
const CDP_HOST = process.env.AISW_CDP_HOST || "127.0.0.1";

export function cdpBase() {
  return `http://${CDP_HOST}:${CDP_PORT}`;
}

export async function listTargets() {
  const response = await fetch(`${cdpBase()}/json`);
  if (!response.ok) {
    throw new Error(`CDP ${cdpBase()}/json → HTTP ${response.status}`);
  }
  return response.json();
}

function isHttpPageUrl(value) {
  try {
    const url = new URL(String(value || ""));
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function originIsUsable(origin) {
  if (!origin || origin === "null") {
    return false;
  }
  try {
    const url = new URL(origin);
    return Boolean(url.protocol) && url.protocol !== "about:";
  } catch {
    return false;
  }
}

export async function pageWebSocketUrl() {
  const targets = await listTargets();
  const pages = targets.filter((target) => target.type === "page" && target.webSocketDebuggerUrl);
  if (pages.length === 0) {
    throw new Error(`no CDP page on ${cdpBase()}/json`);
  }
  const preferred =
    pages.find((page) => isHttpPageUrl(page.url) && /tauri\.localhost|localhost:\d+/i.test(page.url)) ||
    pages.find((page) => isHttpPageUrl(page.url));
  return (preferred || pages[0]).webSocketDebuggerUrl;
}

function connect(wsUrl) {
  const ws = new WebSocket(wsUrl);
  let nextId = 0;
  const pending = new Map();
  const eventHandlers = new Set();
  ws.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (message.id && pending.has(message.id)) {
      const { resolve, reject, timer } = pending.get(message.id);
      clearTimeout(timer);
      pending.delete(message.id);
      if (message.error) {
        reject(new Error(JSON.stringify(message.error)));
      } else {
        resolve(message.result);
      }
      return;
    }
    if (message.method) {
      for (const handler of eventHandlers) {
        try {
          handler(message);
        } catch {
          // event handlers must not break the CDP session
        }
      }
    }
  });
  const ready = new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve);
    ws.addEventListener("error", () => reject(new Error("cdp websocket error")));
  });
  async function send(method, params = {}, timeoutMs = 20_000) {
    await ready;
    const id = ++nextId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`CDP timeout ${method}`)), timeoutMs);
      pending.set(id, { resolve, reject, timer });
      ws.send(JSON.stringify({ id, method, params }));
    });
  }
  function onEvent(handler) {
    eventHandlers.add(handler);
    return () => eventHandlers.delete(handler);
  }
  return { ws, send, ready, onEvent };
}

async function continuePausedRequest(send, params, fallbackOrigin) {
  const requestId = params?.requestId;
  if (!requestId) {
    return;
  }
  const headers = [];
  for (const [name, value] of Object.entries(params.request?.headers || {})) {
    headers.push({ name, value: String(value) });
  }
  const originHeader = headers.find((header) => header.name.toLowerCase() === "origin");
  const originValue = originHeader?.value || "";
  if (originIsUsable(originValue)) {
    await send("Fetch.continueRequest", { requestId });
    return;
  }
  if (originHeader) {
    originHeader.value = fallbackOrigin;
  } else {
    headers.push({ name: "Origin", value: fallbackOrigin });
  }
  await send("Fetch.continueRequest", { requestId, headers });
}

function unwrapEvaluate(result) {
  if (result?.exceptionDetails) {
    const detail = result.exceptionDetails;
    const text =
      detail.exception?.description ||
      detail.exception?.value ||
      detail.text ||
      JSON.stringify(detail);
    throw new Error(String(text));
  }
  const value = result?.result?.value;
  if (value && typeof value === "object" && value.__aiswError) {
    throw new Error(value.__aiswError);
  }
  return value;
}

export async function withCdp(body) {
  const wsUrl = await pageWebSocketUrl();
  const session = connect(wsUrl);
  await session.ready;
  try {
    return await body(session.send, session);
  } finally {
    session.ws.close();
  }
}

export async function waitForTauri(timeoutMs = 45_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError = "tauri internals not ready";
  while (Date.now() < deadline) {
    try {
      const snapshot = await withCdp(async (send) => {
        const result = await send("Runtime.evaluate", {
          expression: `(() => ({
            hasInvoke: Boolean(globalThis.__TAURI_INTERNALS__ && typeof globalThis.__TAURI_INTERNALS__.invoke === "function"),
            origin: String(location.origin || ""),
            href: String(location.href || ""),
          }))()`,
          returnByValue: true,
        });
        return unwrapEvaluate(result);
      });
      if (!snapshot?.hasInvoke) {
        throw new Error("invoke missing");
      }
      const href = String(snapshot.href || "");
      const loaded = href.length > 0 && !href.startsWith("about:");
      if (!loaded && !originIsUsable(snapshot.origin)) {
        throw new Error(`page not loaded origin=${snapshot.origin || "(empty)"} href=${href || "(empty)"}`);
      }
      return snapshot;
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error);
      await new Promise((resolve) => setTimeout(resolve, 400));
    }
  }
  throw new Error(`Tauri IPC not ready: ${lastError}`);
}

export async function invoke(command, args, timeoutMs = 30_000) {
  return withCdp(async (send, session) => {
    const locationResult = await send("Runtime.evaluate", {
      expression: "String(location.origin || '')",
      returnByValue: true,
    });
    const pageOrigin = unwrapEvaluate(locationResult);
    const fallbackOrigin = originIsUsable(pageOrigin) ? pageOrigin : "https://tauri.localhost";
    const stopEvents = session.onEvent((message) => {
      if (message.method !== "Fetch.requestPaused") {
        return;
      }
      continuePausedRequest(send, message.params, fallbackOrigin).catch(() => {});
    });
    await send("Fetch.enable", {
      patterns: [{ urlPattern: "*", requestStage: "Request" }],
    });
    try {
      const expression = `(() => {
        const internals = globalThis.__TAURI_INTERNALS__;
        if (!internals || typeof internals.invoke !== "function") {
          return Promise.resolve({ __aiswError: "TAURI_INTERNALS missing" });
        }
        return internals.invoke(${JSON.stringify(command)}, ${JSON.stringify(args ?? null) === "null" ? "undefined" : JSON.stringify(args)}).then(
          (value) => value,
          (error) => ({
            __aiswError:
              (error && (error.message || error)) ? String(error.message || error) : String(error),
          })
        );
      })()`;
      const result = await send(
        "Runtime.evaluate",
        { expression, awaitPromise: true, returnByValue: true },
        timeoutMs
      );
      return unwrapEvaluate(result);
    } finally {
      stopEvents();
      try {
        await send("Fetch.disable");
      } catch {
        // session may already be closing
      }
    }
  });
}

export async function evaluate(expression, timeoutMs = 15_000) {
  return withCdp(async (send) => {
    const result = await send(
      "Runtime.evaluate",
      { expression, awaitPromise: true, returnByValue: true },
      timeoutMs
    );
    return unwrapEvaluate(result);
  });
}

function parseCliArgs(argv) {
  const command = argv[0];
  if (!command) {
    throw new Error("usage: node cdp-invoke.mjs <command> [jsonArgs|positional...]");
  }
  const rest = argv.slice(1);
  if (rest.length === 0) {
    return { command, args: undefined };
  }
  if (rest.length === 1) {
    const token = rest[0];
    try {
      return { command, args: JSON.parse(token) };
    } catch {
      return { command, args: token };
    }
  }
  try {
    return { command, args: JSON.parse(rest.join(" ")) };
  } catch {
    return { command, args: rest };
  }
}

const isCli = /cdp-invoke\.mjs$/i.test(String(process.argv[1] || "").replaceAll("\\", "/"));

if (isCli) {
  const { command, args } = parseCliArgs(process.argv.slice(2));
  const payload =
    args === undefined
      ? undefined
      : typeof args === "string" || Array.isArray(args)
        ? args
        : args;
  waitForTauri()
    .then(() => invoke(command, payload))
    .then((value) => {
      console.log(JSON.stringify(value, null, 2));
    })
    .catch((error) => {
      console.error(error instanceof Error ? error.message : error);
      process.exit(1);
    });
}
