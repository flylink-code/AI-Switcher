// 仅探测本机 Vite 页面；不继承系统 HTTP 代理，也不接受错误页。
const port = Number(process.argv[2]);
const timeoutMs = Number(process.argv[3] || 5000);
if (!Number.isInteger(port) || port < 1024 || port > 65535) {
  throw new Error("无效的 Vite 端口");
}
try {
  const response = await fetch(`http://127.0.0.1:${port}/`, {
    signal: AbortSignal.timeout(timeoutMs),
    redirect: "error",
  });
  const html = await response.text();
  if (!response.ok || !html.includes("/@vite/client") || !html.includes('id="root"')) {
    throw new Error(`不是应用 Vite 页面：HTTP ${response.status}`);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
