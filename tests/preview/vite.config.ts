import { defineConfig } from "vite";
import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
export default defineConfig({
  root: "tests/preview", publicDir: "../../ui/public", plugins: [react(), tailwindcss()],
  resolve: { alias: [{ find: /^(?:\.{1,2}\/lib|\.)\/tauri$/, replacement: resolve(import.meta.dirname, "tauri.ts") }] },
  server: { host: "127.0.0.1", port: 5174, strictPort: true },
});
