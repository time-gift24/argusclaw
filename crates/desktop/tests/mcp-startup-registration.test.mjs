import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const tauriAppSource = readFileSync(
  new URL("../src-tauri/src/lib.rs", import.meta.url),
  "utf8",
);

test("mcp tool registration runs in a spawned task instead of blocking app startup", () => {
  assert.match(tauriAppSource, /runtime\.spawn\(\s*async move \{/);
  assert.match(tauriAppSource, /register_mcp_tools\(\)\.await/);
  assert.doesNotMatch(
    tauriAppSource,
    /block_on\(wing\.register_mcp_tools\(\)\)/,
  );
});
