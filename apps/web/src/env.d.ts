/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";

  const component: DefineComponent<Record<string, never>, Record<string, never>, unknown>;
  export default component;
}

declare module "@opentiny/tiny-robot-svgs/dist/tiny-robot-svgs.js" {
  export * from "@opentiny/tiny-robot-svgs/dist/index";
}
