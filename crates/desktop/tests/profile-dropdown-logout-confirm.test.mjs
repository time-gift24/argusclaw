import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const profileDropdownPath = new URL(
  "../components/shadcn-studio/blocks/dropdown-profile.tsx",
  import.meta.url,
);

test("profile trigger is reduced to a logout confirmation dialog", () => {
  const source = readFileSync(profileDropdownPath, "utf8");

  assert.doesNotMatch(source, /DropdownMenu/);
  assert.doesNotMatch(source, /账号资料|团队管理|界面定制|添加团队账号|账单|设置/);
  assert.match(source, /<Dialog /);
  assert.match(source, /确认退出登录/);
  assert.match(source, /取消/);
  assert.match(source, /退出登录/);
});
