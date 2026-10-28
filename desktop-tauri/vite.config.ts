import { resolve } from "node:path";
import { defineConfig } from "vitest/config";

// Two entry pages: the studio window and the click-through ring light overlay.
export default defineConfig({
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    minify: process.env.TAURI_DEBUG ? false : "esbuild",
    sourcemap: Boolean(process.env.TAURI_DEBUG),
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        ringLight: resolve(__dirname, "ring-light.html"),
      },
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
