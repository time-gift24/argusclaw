import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const packageJson = JSON.parse(
  readFileSync(new URL("../package.json", import.meta.url), "utf8"),
);

test("desktop next scripts opt into webpack for tauri compatibility", () => {
  assert.match(packageJson.scripts.dev, /--webpack/);
  assert.match(packageJson.scripts.build, /--webpack/);
  assert.equal(packageJson.scripts.lint, "eslint .");
});
