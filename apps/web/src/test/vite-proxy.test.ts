// @vitest-environment node
import { describe, expect, it } from "vitest";

import { createDevServerProxy, resolveApiProxyTarget } from "../../vite.config";

describe("resolveApiProxyTarget", () => {
  it("defaults to the argus-server dev port", () => {
    expect(resolveApiProxyTarget({})).toBe("http://127.0.0.1:3000");
  });

  it("prefers ARGUS_SERVER_URL when provided", () => {
    expect(
      resolveApiProxyTarget({
        ARGUS_SERVER_URL: "http://127.0.0.1:4010",
      }),
    ).toBe("http://127.0.0.1:4010");
  });

  it("proxies API and auth routes to the argus-server target", () => {
    const proxy = createDevServerProxy("http://127.0.0.1:4010");

    expect(proxy["/api/v1"]?.target).toBe("http://127.0.0.1:4010");
    expect(proxy["/auth"]?.target).toBe("http://127.0.0.1:4010");
  });
});
