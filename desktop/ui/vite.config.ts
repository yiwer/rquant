import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "node:path";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@bindings": path.resolve(__dirname, "../src-tauri/bindings") },
  },
  server: { port: 5173, strictPort: true },
  // vitest
  test: {
    environment: "jsdom",
    globals: true,
  },
});
