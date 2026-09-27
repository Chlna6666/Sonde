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
          const normalizedId = id.replace(/\\/g, "/");

          if (normalizedId.includes("locales/zh-CN.json")) {
            return "locale-zh-CN";
          }
          if (normalizedId.includes("locales/en.json")) {
            return "locale-en";
          }
          if (
            normalizedId.includes("node_modules/react/") ||
            normalizedId.includes("node_modules/react-dom/") ||
            normalizedId.includes("node_modules/react-router/") ||
            normalizedId.includes("node_modules/react-router-dom/") ||
            normalizedId.includes("node_modules/scheduler/")
          ) {
            return "vendor-react";
          }
          if (
            normalizedId.includes("node_modules/i18next/") ||
            normalizedId.includes("node_modules/react-i18next/")
          ) {
            return "vendor-i18n";
          }
          if (
            normalizedId.includes("node_modules/lucide-react/") ||
            normalizedId.includes("node_modules/react-icons/")
          ) {
            return "vendor-icons";
          }
          if (
            normalizedId.includes("node_modules/@radix-ui/") ||
            normalizedId.includes("node_modules/class-variance-authority/") ||
            normalizedId.includes("node_modules/tailwind-merge/") ||
            normalizedId.includes("node_modules/clsx/")
          ) {
            return "vendor-ui";
          }
          if (normalizedId.includes("node_modules/motion/")) {
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


