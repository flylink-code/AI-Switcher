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

function isChromeErrorUrl(value) {
  const text = String(value || "");
  return (
    text.startsWith("chrome-error://") ||
    text.includes("chromewebdata") ||
    text.startsWith("about:neterror")
  );
}

function originIsUsable(origin) {
  if (!origin || origin === "null") {
    return false;
  }
  try {
    const url = new URL(origin);
    return (
      Boolean(url.protocol) &&
      url.protocol !== "about:" &&
      url.protocol !== "chrome-error:"
    );
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
  const validPages = pages.filter((page) => !isChromeErrorUrl(page.url));
  const candidates = validPages.length > 0 ? validPages : pages;
  const preferred =
    candidates.find((page) => isHttpPageUrl(page.url) && /tauri\.localhost|localhost:\d+|127\.0\.0\.1:\d+/i.test(page.url)) ||
    candidates.find((page) => isHttpPageUrl(page.url));
  return (preferred || candidates[0]).webSocketDebuggerUrl;
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

export async function waitForTauri(timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError = "tauri internals not ready";
  let reloadAttempted = false;
  // 持续监听整个启动过程，避免每次轮询断开时丢失模块加载错误。
  return withCdp(async (send, session) => {
    const diagnostics = [];
    const pendingRequests = new Map();
    const stopDiagnostics = session.onEvent((message) => {
      const params = message.params || {};
      if (message.method === "Network.requestWillBeSent") {
        pendingRequests.set(params.requestId, params.request?.url || "");
      } else if (message.method === "Network.loadingFinished" || message.method === "Network.loadingFailed") {
        const url = pendingRequests.get(params.requestId) || "";
        pendingRequests.delete(params.requestId);
        if (message.method === "Network.loadingFailed") {
          diagnostics.push({ type: "network", url, error: params.errorText });
        }
      } else if (message.method === "Runtime.exceptionThrown") {
        const detail = params.exceptionDetails || {};
        diagnostics.push({ type: "exception", text: detail.exception?.description || detail.text });
      } else if (message.method === "Log.entryAdded") {
        diagnostics.push({ type: "log", text: String(params.entry?.text || "").slice(0, 500) });
      }
      if (diagnostics.length > 40) diagnostics.shift();
    });
    await send("Runtime.enable");
    await send("Log.enable").catch(() => {});
    await send("Network.enable");
    try {
  while (Date.now() < deadline) {
    try {
      const snapshot = await (async () => {
        const result = await send("Runtime.evaluate", {
          expression: `(() => {
            const href = String(location.href || "");
            const origin = String(location.origin || "");
            const title = String(document.title || "");
            const isChromeError =
              href.startsWith("chrome-error://") ||
              href.includes("chromewebdata") ||
              origin.startsWith("chrome-error") ||
              title.includes("ERR_") ||
              title === "Error";

            const root = document.getElementById("root");
            const bootScreen = document.querySelector(".boot-screen");
            const nav = document.querySelector(
              ".v2-top-nav, .app-layout, .ant-layout, [role='navigation']"
            );
            const bodyText = String(document.body?.innerText || "").slice(0, 240);
            const rootMarkup = String(root?.outerHTML || "").slice(0, 600);
            const scriptCount = document.scripts.length;
            const readyState = document.readyState;
            const hasRootChildren = Boolean(
              root && root.children && root.children.length > 0 && !bootScreen
            );
            const uiRendered = Boolean(nav || hasRootChildren);

            return {
              hasInvoke: Boolean(
                globalThis.__TAURI_INTERNALS__ &&
                typeof globalThis.__TAURI_INTERNALS__.invoke === "function"
              ),
              origin,
              href,
              title,
              isChromeError,
              uiRendered,
              readyState,
              scriptCount,
              rootChildCount: root?.children?.length ?? 0,
              hasBootScreen: Boolean(bootScreen),
              hasNavigation: Boolean(nav),
              bodyText,
              rootMarkup,
              resources: performance.getEntriesByType('resource').slice(-12).map(r => ({name:r.name,duration:Math.round(r.duration)})),
            };
          })()`,
          returnByValue: true,
        });
        const evalVal = unwrapEvaluate(result);
        if (evalVal?.isChromeError) {
          try {
            await send("Page.reload", { ignoreCache: true });
          } catch {
            // ignore reload failures on error page
          }
        }
        if (evalVal && typeof evalVal === "object") {
          evalVal.diagnostics = diagnostics.slice(-20);
          evalVal.pendingRequests = [...pendingRequests.values()].slice(-30);
        }
        return evalVal;
      })();

      if (snapshot?.isChromeError) {
        throw new Error(
          `webview loaded chrome-error page (${snapshot.href || snapshot.title || "error"})`
        );
      }
      if (!snapshot?.hasInvoke) {
        throw new Error(`invoke missing: ${JSON.stringify(snapshot)}`);
      }
      const href = String(snapshot.href || "");
      const loaded =
        href.length > 0 &&
        !href.startsWith("about:") &&
        !isChromeErrorUrl(href);
      if (!loaded && !originIsUsable(snapshot.origin)) {
        throw new Error(
          `page not loaded origin=${snapshot.origin || "(empty)"} href=${href || "(empty)"}`
        );
      }
      if (!snapshot?.uiRendered) {
        if (!reloadAttempted && snapshot.readyState === "complete" && pendingRequests.size === 0) {
          reloadAttempted = true;
          await send("Page.reload", { ignoreCache: true });
        }
        throw new Error(`UI not rendered: ${JSON.stringify(snapshot)}`);
      }
      return snapshot;
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error);
      await new Promise((resolve) => setTimeout(resolve, 400));
    }
  }
  throw new Error(`Tauri IPC not ready: ${lastError}`);
    } finally {
      stopDiagnostics();
    }
  });
}

// 保持发起 reload 的 CDP 会话，直到新文档提交，避免短连接提前关闭。
export async function reloadPage(timeoutMs = 30_000) {
  await withCdp(async (send) => {
    await send("Page.enable");
    const marker = `reload-${Date.now()}-${Math.random()}`;
    await send("Runtime.evaluate", {
      expression: `globalThis.__aiswReloadMarker = ${JSON.stringify(marker)}`,
    });
    await send("Page.reload", { ignoreCache: true });
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      try {
        const result = await send("Runtime.evaluate", {
          expression: `globalThis.__aiswReloadMarker !== ${JSON.stringify(marker)}`,
          returnByValue: true,
        });
        if (unwrapEvaluate(result)) return;
      } catch {
        // 导航期间执行上下文会短暂销毁。
      }
      await new Promise((resolve) => setTimeout(resolve, 150));
    }
    throw new Error("页面重载未提交新文档");
  });
  return waitForTauri();
}

let uiOriginBridgeActive = false;

// DOM 点击触发的 IPC 也需要 Origin 修正，而不仅是测试脚本直接 invoke。
export async function withUiOriginBridge(body) {
  return withCdp(async (send, session) => {
    const stop = session.onEvent((event) => {
      if (event.method === "Fetch.requestPaused") {
        continuePausedRequest(send, event.params, "http://localhost").catch(() => {});
      }
    });
    await send("Fetch.enable", { patterns: [{ urlPattern: "*ipc.localhost*", requestStage: "Request" }] });
    uiOriginBridgeActive = true;
    try { return await body(); }
    finally {
      uiOriginBridgeActive = false;
      await send("Fetch.disable").catch(() => {});
      stop();
    }
  });
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
    if (!uiOriginBridgeActive) await send("Fetch.enable", {
      patterns: [{ urlPattern: "*ipc.localhost*", requestStage: "Request" }],
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
        if (!uiOriginBridgeActive) await send("Fetch.disable");
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
