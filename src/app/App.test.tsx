import { describe, expect, it } from "vitest";
import { App } from "./App";

describe("App", () => {
  it("is configured for the Move Party foundation", () => {
    expect(App).toBeTypeOf("function");
  });
});
