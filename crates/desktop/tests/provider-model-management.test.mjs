import test from "node:test"
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { join } from "node:path"

const source = readFileSync(
  join(process.cwd(), "components/settings/provider-form-dialog.tsx"),
  "utf8",
)

test("provider form keeps draft model state for unsaved providers", () => {
  assert.match(
    source,
    /const \[draftModels, setDraftModels\]/,
    "ProviderFormDialog should track draft models before the provider is persisted",
  )
})

test("provider form does not hide model management behind savedProviderId", () => {
  assert.doesNotMatch(
    source,
    /\{savedProviderId && \(/,
    "ProviderFormDialog should expose model management before the provider is saved",
  )
})
