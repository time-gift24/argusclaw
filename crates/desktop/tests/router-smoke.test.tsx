import test from "node:test";
import assert from "node:assert/strict";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createMemoryRouter, RouterProvider } from "react-router-dom";

const matchMedia = () => ({
  matches: false,
  addEventListener: () => {},
  removeEventListener: () => {},
});

const localStorageStub = {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
};

Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: {
    __TAURI_INTERNALS__: {
      invoke: async () => null,
      transformCallback: () => 0,
    },
    localStorage: localStorageStub,
    matchMedia,
  },
});

Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: localStorageStub,
});

Object.defineProperty(globalThis, "document", {
  configurable: true,
  value: {
    documentElement: {
      classList: {
        add: () => {},
        remove: () => {},
        contains: () => false,
        toggle: () => {},
      },
    },
  },
});

async function renderPath(path: string) {
  const { desktopRoutes } = await import("../router");
  const router = createMemoryRouter(desktopRoutes, {
    initialEntries: [path],
  });

  return renderToStaticMarkup(<RouterProvider router={router} />);
}

test("desktop router resolves the providers settings route inside the app shell", async () => {
  const html = await renderPath("/settings/providers");

  assert.match(html, /ArgusWing/);
  assert.match(
    html,
    /<a class="[^"]*bg-accent text-accent-foreground[^"]*" href="\/settings\/providers"/,
  );
});

test("desktop router resolves the agents settings route inside the app shell", async () => {
  const html = await renderPath("/settings/agents");

  assert.match(html, /ArgusWing/);
  assert.match(
    html,
    /<a class="[^"]*bg-accent text-accent-foreground[^"]*" href="\/settings\/agents"/,
  );
});
