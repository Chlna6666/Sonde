import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:8080",
      "/v1": "http://127.0.0.1:8080"
    }
  },
  build: {
    target: "es2022",
    // Never ship source maps: web/dist is embedded into the server binary and served publicly.
    sourcemap: false,
    cssCodeSplit: true,
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("locales/zh-CN.json") || id.includes("locales\\zh-CN.json")) {
            return "locale-zh-CN";
          }
          if (id.includes("locales/en.json") || id.includes("locales\\en.json")) {
            return "locale-en";
          }
          if (
            id.includes("node_modules/react/") ||
            id.includes("node_modules/react-dom/") ||
            id.includes("node_modules/react-router-dom/")
          ) {
            return "vendor-react";
          }
          if (
            id.includes("node_modules/i18next/") ||
            id.includes("node_modules/react-i18next/")
          ) {
            return "vendor-i18n";
          }
          if (id.includes("node_modules/lucide-react/")) {
            return "vendor-icons";
          }
          if (id.includes("node_modules/recharts/") || id.includes("node_modules/victory-vendor/")) {
            return "vendor-charts";
          }
          if (id.includes("node_modules/motion/")) {
            return "vendor-motion";
          }
        },
        chunkFileNames: "assets/chunk-[name]-[hash].js",
        entryFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash].[ext]"
      }
    }
  }
});


