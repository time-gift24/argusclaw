import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const dropdownMenuPath = new URL(
  "../components/ui/dropdown-menu.tsx",
  import.meta.url,
);

test("dropdown menu label is a plain header element instead of a group label primitive", () => {
  const dropdownMenuSource = readFileSync(dropdownMenuPath, "utf8");

  assert.match(dropdownMenuSource, /function DropdownMenuLabel/);
  assert.doesNotMatch(dropdownMenuSource, /MenuPrimitive\.GroupLabel/);
  assert.match(dropdownMenuSource, /<div[\s\S]*data-slot="dropdown-menu-label"/);
});
