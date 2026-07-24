import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// Tauri dev host (for mobile/remote dev). Empty in normal desktop dev.
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
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
    // Produce a single chunk to simplify Tauri asset loading.
    rollupOptions: {
      output: {
        manualChunks: undefined,
      },
    },
  },
});
