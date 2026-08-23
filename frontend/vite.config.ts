import { defineConfig } from "vite";

// Dev server proxies the game API to the Rust server, so `npm run dev`
// works while `cargo run` is serving on 127.0.0.1:8080.
export default defineConfig({
  server: {
    proxy: {
      "/state": "http://127.0.0.1:8080",
      "/action": "http://127.0.0.1:8080",
      "/ai/analyze": "http://127.0.0.1:8080",
    },
  },
  // Two entry pages: the human-player page and the ai-player page.
  build: {
    rollupOptions: {
      input: {
        main: "index.html",
        ai: "ai.html",
      },
    },
  },
});
