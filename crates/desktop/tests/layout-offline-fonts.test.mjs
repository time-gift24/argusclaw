import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const layoutSource = readFileSync(new URL("../app/layout.tsx", import.meta.url), "utf8");

test("desktop root layout does not depend on next/font/google", () => {
  assert.doesNotMatch(layoutSource, /next\/font\/google/);
});
