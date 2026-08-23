import { defineConfig } from "vite";

/** Resolves an HTML entry to an absolute filesystem path for Rollup's
 * multi-page input, without a Node path import (the config type-checks under
 * the project's tsconfig, which carries no `@types/node`). */
const entry = (file: string): string => new URL(file, import.meta.url).pathname;

// Dev server proxies the game API to the Rust server, so `npm run dev`
// works while `cargo run` is serving on 127.0.0.1:8080.
export default defineConfig({
  server: {
    proxy: {
      "/state": "http://127.0.0.1:8080",
      "/action": "http://127.0.0.1:8080",
    },
  },
  build: {
    rollupOptions: {
      // Multi-entry build: the human page (index.html) and the ai page
      // (ai.html) are separate HTML entries over the one shared game backend.
      input: {
        main: entry("index.html"),
        ai: entry("ai.html"),
      },
    },
  },
});
