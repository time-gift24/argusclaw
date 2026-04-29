# Web Content Width Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Widen large-screen web admin pages to a centered `1600px` content container.

**Architecture:** Update the shared admin layout CSS so the route header and standard page body use the same centered width cap. Rebuild the web app and verify representative pages in the in-app browser.

**Tech Stack:** Vue 3, Vite, scoped CSS, in-app browser verification

---

### Task 1: Update Shared Layout Width

**Files:**
- Modify: `apps/web/src/layouts/AdminLayout.vue`

**Step 1: Update the shared route content container**

Change the standard content rule from a `1200px` cap to a `1600px` cap and add explicit centering:

```css
.route-shell > :not(.route-header) {
  width: 100%;
  max-width: 1600px;
  margin-inline: auto;
}
```

**Step 2: Center the route header with the same width policy**

Add the same centering contract to the header:

```css
.route-header {
  width: 100%;
  max-width: 1600px;
  margin-inline: auto;
}
```

### Task 2: Rebuild And Verify

**Files:**
- Build output: `apps/web/dist/*`

**Step 1: Rebuild the web app**

Run:

```bash
cd apps/web && pnpm build
```

**Step 2: Verify representative pages**

Check:
- `/`
- `/providers`
- `/providers/new` or another narrower form page

Expected:
- overview/list pages are wider on large screens
- route header and body content are centered
- form/edit screens still look intentionally narrower
