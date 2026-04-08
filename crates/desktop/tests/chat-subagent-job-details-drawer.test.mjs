import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const drawerSource = readFileSync(
  new URL("../components/assistant-ui/subagent-job-details-drawer.tsx", import.meta.url),
  "utf8",
);

test("subagent job details drawer uses dialog primitives and renders the core sections", () => {
  assert.match(drawerSource, /DialogContent/);
  assert.match(drawerSource, /right-0 top-0 h-dvh/);
  assert.match(drawerSource, /最终产出|结果摘要|暂无详细结果/);
  assert.match(drawerSource, /任务信息/);
  assert.match(drawerSource, /执行过程/);
  assert.match(drawerSource, /任务已结束，但详细结果暂不可用/);
});
