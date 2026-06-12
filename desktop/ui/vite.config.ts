import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "node:path";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@bindings": path.resolve(__dirname, "../src-tauri/bindings") },
  },
  // host 钉死 IPv4:node17+ 把 localhost 解析为 ::1,vite 只绑 IPv6 而 WebView2 走 IPv4 会被拒
  server: { host: "127.0.0.1", port: 5173, strictPort: true },
  // vitest
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/setupTests.ts"],
  },
});
