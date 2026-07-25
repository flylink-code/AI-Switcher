import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// Tauri dev host (for mobile/remote dev). Empty in normal desktop dev.
const host = process.env.TAURI_DEV_HOST;

/**
 * Ant Design's prebuilt zero-runtime stylesheet scopes CSS variables with
 * extraction-time `css-var-*` class names. Those generated names do not exist
 * in an arbitrary application tree, leaving most component declarations with
 * unresolved variables. Normalize global variables onto :root and component
 * variables onto their stable `.ant-*` selectors during the Vite build.
 */
const normalizeAntdStaticCss = {
  name: "normalize-antd-static-css",
  enforce: "pre" as const,
  transform(code: string, id: string) {
    const moduleId = id.split("?", 1)[0].replaceAll("\\", "/");
    if (!moduleId.endsWith("/antd/dist/antd.css")) return null;

    return {
      code: code
        .replace(/\.css-var-[\w-]+(?=\.[\w-])/g, "")
        .replace(/\.css-var-[\w-]+(?=\s*\{)/g, ":root"),
      map: null,
    };
  },
};

// https://vite.dev/config/
export default defineConfig({
  plugins: [normalizeAntdStaticCss, react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // Tauri requires a deterministic port and supports HMR via TAURI_DEV_HOST.
  clearScreen: false,
  optimizeDeps: {
    // Only scan the main entry; ignore example apps under ./examples.
    entries: ["index.html"],
  },
  server: {
    port: 5173,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? { protocol: "ws", host, port: 5174 }
      : undefined,
    watch: {
      // Avoid watching the Rust backend, which Vite does not build.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // Tauri webview supports modern ES; esbuild handles minification.
    target: "es2021",
    // Split only runtime dependencies that are needed by the shell. Feature
    // components keep Vite's natural page-level chunks, avoiding a monolithic
    // Ant Design vendor bundle and its circular dependencies.
    rollupOptions: {
      output: {
        manualChunks(id) {
          const moduleId = id.replaceAll("\\", "/");
          if (moduleId.includes("/react/") || moduleId.includes("/react-dom/")) {
            return "react-vendor";
          }
          if (moduleId.includes("/i18next/") || moduleId.includes("/react-i18next/")) {
            return "i18n-vendor";
          }
          return undefined;
        },
      },
    },
  },
});
