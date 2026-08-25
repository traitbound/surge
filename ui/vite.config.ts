import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev-only proxy to the running surge-server; in production the built assets
// are embedded in the binary and served from the same origin (ADR-4).
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: { "/healthz": "http://127.0.0.1:7420" },
  },
});
