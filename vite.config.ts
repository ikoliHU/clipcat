import { resolve } from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// A főablak (index.html) és az értesítés (toast.html) külön oldal, közös forrásból
export default defineConfig({
  root: "ui",
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: {
    outDir: "../ui-dist",
    emptyOutDir: true,
    target: "es2022",
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "ui/index.html"),
        toast: resolve(import.meta.dirname, "ui/toast.html"),
      },
    },
  },
});
