import type { ProtocolType } from "@/types/backend";

const PROTOCOL_ENDPOINTS: Record<ProtocolType, string> = {
  anthropic: "/v1/messages",
  proxy: "/v1/chat/completions",
  openai_chat: "/v1/chat/completions",
  openai_responses: "/v1/responses",
};

function isLocalHttpHost(hostname: string): boolean {
  const host = hostname.toLowerCase();
  return host === "localhost" || host === "127.0.0.1" || host === "[::1]" || host === "::1";
}

/** Validate and convert a pasted request endpoint into a reusable Base URL. */
export function normalizeBaseUrl(value: string): string {
  const trimmed = value.trim();
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw new Error("invalidBaseUrl");
  }
  const allowLocalHttp = url.protocol === "http:" && isLocalHttpHost(url.hostname);
  if (url.protocol !== "https:" && !allowLocalHttp) throw new Error("baseUrlMustUseHttps");
  if (url.username || url.password) throw new Error("baseUrlNoCredentials");
  if (url.search || url.hash) throw new Error("baseUrlNoQueryOrFragment");

  let path = url.pathname.replace(/\/+$/, "");
  path = path.replace(/\/(?:chat\/completions|messages|responses|models)$/i, "");
  url.pathname = path.replace(/\/+$/, "") || "/";
  return url.toString().replace(/\/+$/, "");
}

export function needsOpenAiV1Suffix(value: string): boolean {
  try {
    const normalized = normalizeBaseUrl(value);
    const url = new URL(normalized);
    const path = url.pathname.replace(/\/+$/, "") || "/";
    return path === "/";
  } catch {
    return false;
  }
}

export function ensureOpenAiV1Suffix(value: string): string {
  const normalized = normalizeBaseUrl(value);
  return needsOpenAiV1Suffix(normalized) ? `${normalized}/v1` : normalized;
}

export function buildEndpointPreview(baseUrl: string | undefined, protocol: ProtocolType): string {
  if (!baseUrl?.trim()) return "";
  try {
    const base = normalizeBaseUrl(baseUrl);
    const endpoint = PROTOCOL_ENDPOINTS[protocol];
    return `${base}${base.endsWith("/v1") && endpoint.startsWith("/v1/") ? endpoint.slice(3) : endpoint}`;
  } catch {
    return "";
  }
}

/** Upstream pool must not point at local agent proxies or the smart gateway itself. */
export function isReservedListenerUrl(value: string): boolean {
  try {
    const url = new URL(value.trim());
    if (!isLocalHttpHost(url.hostname)) return false;
    const port = url.port ? Number(url.port) : url.protocol === "https:" ? 443 : 80;
    return port >= 15821 && port <= 15828;
  } catch {
    return false;
  }
}
