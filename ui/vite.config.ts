import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev-only proxy to the running surge-server; in production the built assets
// are embedded in the binary and served from the same origin (ADR-4).
// /claim is proxied too so the one-time claim URL sets its session cookie on
// the dev origin (INV-AUTH-5) — visit the printed URL with :5173 as the host.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": "http://127.0.0.1:7420",
      "/runtime": "http://127.0.0.1:7420",
      "/healthz": "http://127.0.0.1:7420",
      "/claim": "http://127.0.0.1:7420",
    },
  },
});
