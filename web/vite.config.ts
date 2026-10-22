import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// In dev, the daemon serves the API; Vite proxies /api to it.
const daemon = process.env.BANKAN_URL ?? "http://127.0.0.1:7777";

export default defineConfig({
  plugins: [react()],
  server: { port: 5173, proxy: { "/api": { target: daemon, changeOrigin: true } } },
  build: { outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 1500 },
});
