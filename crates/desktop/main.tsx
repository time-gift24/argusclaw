import * as React from "react";
import * as ReactDOM from "react-dom/client";
import { RouterProvider } from "react-router-dom";

import "./app/globals.css";
import { createDesktopRouter } from "@/router";

const rootElement = document.getElementById("root");

if (!rootElement) {
  throw new Error("Failed to find the desktop root element.");
}

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <RouterProvider router={createDesktopRouter()} />
  </React.StrictMode>,
);
