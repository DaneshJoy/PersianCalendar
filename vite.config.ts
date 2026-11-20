import { defineConfig } from "vite";
import { resolve } from "node:path";

const rootDirectory = import.meta.dirname;

// Vite config: two entry points (popup flyout + desktop widget) for the
// Tauri v2 shell. `clearScreen: false` keeps cargo output visible in dev.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "chrome110",
    // Vite 8 bundles with Rolldown/Oxc; "oxc" is the supported minifier
    // ("esbuild" would require a separately installed esbuild package).
    minify: "oxc",
    sourcemap: false,
    rollupOptions: {
      input: {
        popup: resolve(rootDirectory, "index.html"),
        widget: resolve(rootDirectory, "widget.html"),
        capsule: resolve(rootDirectory, "capsule.html"),
      },
    },
  },
});
