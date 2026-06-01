import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// During dev, proxy /api to the Rust backend so cookies are same-origin.
// The target is the backend's *local* listen address, derived from BIND_ADDR
// (default 8787). Do NOT use BACKEND_URL here: that's the browser-facing origin
// (:5173 in dev, so the OAuth callback works), and proxying /api there would
// just loop back into Vite.
const backendPort = (process.env.BIND_ADDR || "0.0.0.0:8787").split(":").pop();
const proxyTarget = process.env.VITE_PROXY_TARGET || `http://localhost:${backendPort}`;

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: proxyTarget,
        changeOrigin: false,
      },
    },
  },
});
