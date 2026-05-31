import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// During dev, proxy /api to the Rust backend so cookies are same-origin.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: process.env.BACKEND_URL || "http://localhost:8787",
        changeOrigin: false,
      },
    },
  },
});
