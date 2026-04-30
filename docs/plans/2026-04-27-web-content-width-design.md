# Web Content Width Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the web admin pages feel better on large screens by widening the main content area while keeping it centered.

**Architecture:** Keep the change in the shared admin layout so most pages inherit the new large-screen behavior automatically. Use a larger max width and explicit centering on both the route header and normal page content, while preserving page-level narrower max-width overrides for edit and form screens.

**Tech Stack:** Vue 3, scoped CSS in `apps/web/src/layouts/AdminLayout.vue`

---

## Summary

The current admin shell limits most non-immersive pages to `1200px`, which leaves too much unused space on large displays. The update raises the shared content cap to `1600px` and centers the route header and content blocks with `width: 100%` plus `margin-inline: auto`.

## Intended Outcome

- Large-screen pages become visibly wider
- Content remains centered instead of sticking to the left
- Existing page-local `max-width` rules for narrower edit/import views continue to work
- Immersive routes keep their current full-width behavior
