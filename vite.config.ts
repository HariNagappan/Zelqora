import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  // Prevent Vite from clearing the screen so Tauri/cargo output stays visible.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // The Rust side is watched by cargo; don't watch it from Vite.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // WebView2 is an evergreen Chromium engine, so Vite's default target is
    // fine. Note: Vite 8+ uses Oxc for minification/transpilation — do not set
    // `minify: "esbuild"` or an older explicit target (those require the
    // deprecated standalone esbuild package).
    sourcemap: false,
  },
});
