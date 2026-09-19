import type { AppSnapshot } from "../backend/appRuntime";
import { commandErrorMessage } from "../backend/appRuntime";

/**
 * Whether a snapshot command actually succeeded, and what to tell the user if
 * it did not.
 *
 * Every Movie Party snapshot command returns a bare `AppSnapshot` on the Rust
 * side — never `Option`, never `Result` — so a *resolved* call always yields an
 * object. That is what makes a missing result unambiguous: `null` can only mean
 * the command failed. Treating it as "nothing happened" is exactly what let a
 * failed `leave_party` navigate Home as though the party had ended.
 */
export type SnapshotOutcome =
  | { ok: true; snapshot: AppSnapshot }
  | { ok: false; message: string };

/**
 * Classify one snapshot command result.
 *
 * This is the single place that answers "did it work?", so no call site has to
 * re-derive it — and so no call site can quietly get it wrong again.
 */
export function evaluateSnapshotOutcome(
  result: AppSnapshot | null | undefined,
  error: unknown,
  fallbackMessage: string,
): SnapshotOutcome {
  if (result) {
    return { ok: true, snapshot: result };
  }
  return { ok: false, message: commandErrorMessage(error, fallbackMessage) };
}

/**
 * A monotonic token identifying the user's *current* intent for a multi-step
 * flow (creating a party, joining one).
 *
 * A flow's result may only touch the UI while its token is still current.
 * Starting a new attempt, or navigating anywhere, invalidates the previous
 * token — so a late-arriving result from an abandoned attempt cannot yank the
 * user back to a screen they already left.
 *
 * Deliberately NOT folded into `applySnapshot`: snapshots pushed by the backend
 * are authoritative and must always apply, however late they arrive. The
 * staleness problem is specific to *command results*, which represent an intent
 * the user may have since abandoned.
 */
export type FlowToken = {
  /** Claim the flow for a new attempt. Invalidates every earlier token. */
  begin: () => number;
  /** Abandon the flow: no outstanding token is current any more. */
  invalidate: () => void;
  /** True while `token` is still the newest claim on the flow. */
  isCurrent: (token: number) => boolean;
};

export function createFlowToken(): FlowToken {
  let current = 0;
  return {
    begin: () => {
      current += 1;
      return current;
    },
    invalidate: () => {
      current += 1;
    },
    isCurrent: (token: number) => token === current,
  };
}
