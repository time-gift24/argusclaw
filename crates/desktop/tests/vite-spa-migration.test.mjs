import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const packageJson = JSON.parse(
  readFileSync(new URL("../package.json", import.meta.url), "utf8"),
);
const tauriConfig = JSON.parse(
  readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
);
const tsconfig = JSON.parse(
  readFileSync(new URL("../tsconfig.json", import.meta.url), "utf8"),
);

test("desktop build scripts target a Vite-powered Tauri SPA", () => {
  assert.equal(packageJson.scripts.dev, "vite");
  assert.match(packageJson.scripts.build, /^vite build$/);
  assert.equal(packageJson.scripts.preview, "vite preview");
  assert.equal(packageJson.scripts.tauri, "node ./scripts/tauri.mjs");
  assert.equal(packageJson.dependencies.next, undefined);
  assert.equal(packageJson.dependencies["next-themes"], undefined);
  assert.ok(packageJson.dependencies["react-router-dom"]);
  assert.ok(packageJson.devDependencies.vite);
  assert.ok(packageJson.devDependencies["@vitejs/plugin-react"]);

  const tauriWrapperPath = new URL("../scripts/tauri.mjs", import.meta.url);
  assert.equal(existsSync(tauriWrapperPath), true);
});

test("desktop tauri config points at the Vite dev server and dist output", () => {
  assert.equal(tauriConfig.build.beforeDevCommand, "pnpm dev");
  assert.equal(tauriConfig.build.beforeBuildCommand, "pnpm build");
  assert.equal(tauriConfig.build.devUrl, "http://localhost:5173");
  assert.equal(tauriConfig.build.frontendDist, "../dist");
});

test("desktop TypeScript and Vite config no longer depend on Next.js", () => {
  const viteConfigPath = new URL("../vite.config.ts", import.meta.url);
  assert.equal(existsSync(viteConfigPath), true);

  const viteConfigSource = readFileSync(viteConfigPath, "utf8");

  assert.match(viteConfigSource, /defineConfig/);
  assert.match(viteConfigSource, /@vitejs\/plugin-react/);
  assert.match(viteConfigSource, /port:\s*5173/);
  assert.match(viteConfigSource, /strictPort:\s*true/);
  assert.match(viteConfigSource, /ignored:\s*\[\s*['"]\*\*\/src-tauri\/\*\*['"]\s*\]/);
  assert.doesNotMatch(viteConfigSource, /next/i);
  assert.ok(!tsconfig.compilerOptions.plugins);
  assert.ok(tsconfig.include.includes("vite-env.d.ts"));
  assert.ok(!tsconfig.include.includes("next-env.d.ts"));
});
