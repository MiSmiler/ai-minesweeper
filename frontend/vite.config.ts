import { defineConfig } from "vite";

// Dev server proxies the game + AI guide API to the Rust server, so
// `npm run dev` works while `cargo run` is serving on 127.0.0.1:8080.
export default defineConfig({
  server: {
    proxy: {
      "/state": "http://127.0.0.1:8080",
      "/action": "http://127.0.0.1:8080",
      "/ai": "http://127.0.0.1:8080",
    },
  },
});
