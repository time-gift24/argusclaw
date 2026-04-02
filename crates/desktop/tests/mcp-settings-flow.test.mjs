import test from "node:test"
import assert from "node:assert/strict"
import { existsSync, readFileSync } from "node:fs"

const dashboardShellSource = readFileSync(
  new URL(
    "../components/shadcn-studio/blocks/dashboard-shell-05/index.tsx",
    import.meta.url,
  ),
  "utf8",
)
const navbarSource = readFileSync(
  new URL(
    "../components/shadcn-studio/blocks/navbar-component-06/navbar-component-06.tsx",
    import.meta.url,
  ),
  "utf8",
)
const mcpPageSource = readFileSync(
  new URL("../app/settings/mcp/page.tsx", import.meta.url),
  "utf8",
)
const mcpEditorSource = readFileSync(
  new URL("../components/settings/mcp-editor.tsx", import.meta.url),
  "utf8",
)
const mcpPagePath = new URL("../app/settings/mcp/page.tsx", import.meta.url)
const newMcpPagePath = new URL("../app/settings/mcp/new/page.tsx", import.meta.url)
const editMcpPagePath = new URL("../app/settings/mcp/edit/page.tsx", import.meta.url)
const mcpEditorPath = new URL("../components/settings/mcp-editor.tsx", import.meta.url)
const mcpCardPath = new URL("../components/settings/mcp-card.tsx", import.meta.url)
const mcpTestDialogPath = new URL(
  "../components/settings/mcp-test-dialog.tsx",
  import.meta.url,
)

test("settings exposes dedicated MCP routes and editor components", () => {
  assert.equal(existsSync(mcpPagePath), true)
  assert.equal(existsSync(newMcpPagePath), true)
  assert.equal(existsSync(editMcpPagePath), true)
  assert.equal(existsSync(mcpEditorPath), true)
  assert.equal(existsSync(mcpCardPath), true)
  assert.equal(existsSync(mcpTestDialogPath), true)

  const newMcpPageSource = readFileSync(newMcpPagePath, "utf8")
  const editMcpPageSource = readFileSync(editMcpPagePath, "utf8")

  assert.match(newMcpPageSource, /<McpEditor[\s\S]*\/>/)
  assert.doesNotMatch(newMcpPageSource, /serverId=/)
  assert.match(editMcpPageSource, /useSearchParams/)
  assert.match(editMcpPageSource, /<McpEditor serverId=\{serverId\} \/>/)
  assert.match(mcpPageSource, /<McpCard/)
  assert.match(mcpPageSource, /<McpTestDialog/)
  assert.match(mcpEditorSource, /<McpTestDialog/)
  assert.match(mcpEditorSource, /const \[headerRows, setHeaderRows\] = React\.useState/)
  assert.match(mcpEditorSource, /Header 名称/)
  assert.match(mcpEditorSource, /Header 值/)
  assert.match(mcpEditorSource, /addHeaderRow/)
})

test("settings navigation and breadcrumbs surface the MCP configuration section", () => {
  assert.match(dashboardShellSource, /href="\/settings\/mcp"/)
  assert.match(dashboardShellSource, /MCP 配置/)
  assert.match(navbarSource, /pathname\.startsWith\("\/settings\/mcp"\)/)
  assert.match(navbarSource, /label: "MCP"/)
})
