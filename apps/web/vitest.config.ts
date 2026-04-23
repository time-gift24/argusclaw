import path from "node:path";
import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      "@opentiny/vue-button": path.resolve(__dirname, "./src/test/stubs/opentiny-button.ts"),
      "@opentiny/vue-card": path.resolve(__dirname, "./src/test/stubs/opentiny-container.ts"),
      "@opentiny/vue-col": path.resolve(__dirname, "./src/test/stubs/opentiny-container.ts"),
      "@opentiny/vue-form": path.resolve(__dirname, "./src/test/stubs/opentiny-container.ts"),
      "@opentiny/vue-form-item": path.resolve(__dirname, "./src/test/stubs/opentiny-container.ts"),
      "@opentiny/vue-grid": path.resolve(__dirname, "./src/test/stubs/opentiny-grid.ts"),
      "@opentiny/vue-grid-column": path.resolve(__dirname, "./src/test/stubs/opentiny-grid-column.ts"),
      "@opentiny/vue-input": path.resolve(__dirname, "./src/test/stubs/opentiny-input.ts"),
      "@opentiny/vue-loading": path.resolve(__dirname, "./src/test/stubs/opentiny-container.ts"),
      "@opentiny/vue-numeric": path.resolve(__dirname, "./src/test/stubs/opentiny-input.ts"),
      "@opentiny/vue-option": path.resolve(__dirname, "./src/test/stubs/opentiny-option.ts"),
      "@opentiny/vue-row": path.resolve(__dirname, "./src/test/stubs/opentiny-container.ts"),
      "@opentiny/vue-select": path.resolve(__dirname, "./src/test/stubs/opentiny-select.ts"),
      "@opentiny/vue-switch": path.resolve(__dirname, "./src/test/stubs/opentiny-switch.ts"),
      "@opentiny/vue-tag": path.resolve(__dirname, "./src/test/stubs/opentiny-container.ts"),
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
  },
});
