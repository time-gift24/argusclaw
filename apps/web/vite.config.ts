import path from "node:path";
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig(({ mode }) => {
  const isTest = mode === "test" || process.env.VITEST === "true";
  const apiProxyTarget = process.env.ARGUS_SERVER_URL ?? "http://127.0.0.1:3010";

  return {
    define: {
      "process.env.TINY_MODE": JSON.stringify("pc"),
    },
    plugins: [vue()],
    resolve: {
      alias: [
        {
          find: /^@opentiny\/vue-renderless\/(.+)\/vue$/,
          replacement: "@opentiny/vue-renderless/$1/vue.js",
        },
        ...(isTest
          ? [
              {
                find: "@/lib/opentiny",
                replacement: path.resolve(__dirname, "./src/test/stubs/opentiny.ts"),
              },
            ]
          : []),
        {
          find: "@",
          replacement: path.resolve(__dirname, "./src"),
        },
      ],
    },
    server: {
      port: 4173,
      strictPort: true,
      proxy: {
        "/api/v1": {
          target: apiProxyTarget,
          changeOrigin: true,
        },
      },
    },
    test: {
      environment: "jsdom",
      globals: true,
      setupFiles: ["./src/test/setup.ts"],
      css: true,
    },
  };
});
