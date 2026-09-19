import { describe, expect, it } from "vitest";

import type { AppSnapshot } from "../backend/appRuntime";
import { BackendCommandError } from "../backend/appRuntime";
import { createFlowToken, evaluateSnapshotOutcome } from "./commandOutcome";

/** Only `screen` matters to these decisions; the rest is not under test. */
function snapshot(screen: string): AppSnapshot {
  return { screen } as unknown as AppSnapshot;
}

describe("evaluateSnapshotOutcome", () => {
  it("treats a returned snapshot as success", () => {
    const result = evaluateSnapshotOutcome(snapshot("HOME"), null, "fallback");

    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.snapshot.screen).toBe("HOME");
    }
  });

  /**
   * MP-02: a null result is a FAILURE, not a no-op.
   *
   * The snapshot commands return a bare `AppSnapshot` on the Rust side, so a
   * resolved call always yields an object. Before this, `applySnapshot(null)`
   * silently did nothing and the caller went on to navigate Home — so a failed
   * leave looked exactly like a successful one.
   */
  it("treats a null result as a failure rather than nothing happening", () => {
    const result = evaluateSnapshotOutcome(null, null, "Could not leave the party.");

    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toBe("Could not leave the party.");
    }
  });

  it("treats an undefined result as a failure too", () => {
    const result = evaluateSnapshotOutcome(undefined, null, "Could not leave the party.");

    expect(result.ok).toBe(false);
  });

  it("prefers the command's own error over the fallback", () => {
    const error = new BackendCommandError("leave_party", "MP-STORE-001 save failed");

    const result = evaluateSnapshotOutcome(null, error, "Could not leave the party.");

    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toContain("MP-STORE-001");
      expect(result.message).not.toBe("Could not leave the party.");
    }
  });

  it("ignores an error when the command actually returned a snapshot", () => {
    const result = evaluateSnapshotOutcome(snapshot("LOBBY"), new Error("stale"), "fallback");

    expect(result.ok).toBe(true);
  });
});

describe("createFlowToken", () => {
  it("keeps the newest claim current", () => {
    const flow = createFlowToken();

    const token = flow.begin();

    expect(flow.isCurrent(token)).toBe(true);
  });

  /**
   * MP-05: starting a second attempt must invalidate the first, so a late
   * result from the abandoned attempt cannot overwrite the newer intent.
   */
  it("invalidates an earlier attempt when a new one begins", () => {
    const flow = createFlowToken();

    const first = flow.begin();
    const second = flow.begin();

    expect(flow.isCurrent(first)).toBe(false);
    expect(flow.isCurrent(second)).toBe(true);
  });

  /** MP-05: navigating away abandons the flow outright. */
  it("invalidates every outstanding token when the flow is abandoned", () => {
    const flow = createFlowToken();

    const token = flow.begin();
    flow.invalidate();

    expect(flow.isCurrent(token)).toBe(false);
  });

  it("does not treat an unrelated token number as current", () => {
    const flow = createFlowToken();
    flow.begin();

    expect(flow.isCurrent(0)).toBe(false);
    expect(flow.isCurrent(999)).toBe(false);
  });
});
