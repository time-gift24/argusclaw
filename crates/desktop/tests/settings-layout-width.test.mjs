import test from "node:test"
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { join } from "node:path"

const desktopRoot = process.cwd()

function readSource(relativePath) {
  return readFileSync(join(desktopRoot, relativePath), "utf8")
}

function expectFullWidthContainer(relativePath) {
  const source = readSource(relativePath)
  assert.match(
    source,
    /className="[^"]*\bw-full\b[^"]*\bmax-w-7xl\b[^"]*"/,
    `${relativePath} should keep the settings container at full width before max-width is applied`,
  )
}

test("agents settings page keeps the shared layout width", () => {
  expectFullWidthContainer("app/settings/agents/page.tsx")
})

test("providers settings page keeps the shared layout width", () => {
  expectFullWidthContainer("app/settings/providers/page.tsx")
})
