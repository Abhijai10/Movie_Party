import { useEffect, useState } from "react";
import { AlertTriangle, ChevronDown, RotateCcw } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";

/**
 * UI_UX_SPEC §64 — the Error screen.
 *
 * Stable MP code + human message + actionable options + collapsible
 * Technical Details. §23: the stable code is the contract; raw
 * internals belong in the details, never replacing the code.
 *
 * Every recoverable error offers a useful action — there is no dead-end
 * state (§64: "Every recoverable error should offer useful action").
 */
export type ErrorScreenAction = {
  label: string;
  onClick: () => void;
  variant?: "primary" | "ghost" | "neutral";
};

type ErrorScreenViewProps = {
  /** Stable Movie Party code, e.g. "MP-NET-003". Empty hides the code row. */
  code: string;
  /** Human explanation — one plain sentence, never raw internals. */
  message: string;
  /** Recoverable actions offered to the user (at least one per §64). */
  actions: ErrorScreenAction[];
  /** Raw diagnostic detail for the collapsible section. */
  technicalDetails?: string;
  onDismiss?: () => void;
};

/** Extract the stable MP code from a backend error string, if any. */
export function extractMpCode(error: string | null | undefined): string {
  if (!error) return "";
  return error.match(/\bMP-[A-Z]+(?:-[A-Z0-9]+)*-\d{3}\b/)?.[0] ?? "";
}

/**
 * Map an MP code family to a human message the user can act on. Codes are
 * the contract (§23); this table keeps user copy honest and stable.
 */
export function humanMessageForCode(code: string): string {
  if (code.startsWith("MP-NET-TS")) {
    return "Movie Party needs Tailscale to reach your partner privately. Open the Tailscale app and connect.";
  }
  if (code.startsWith("MP-NET-")) {
    return "The current connection is too slow or unreachable for this session.";
  }
  if (code.startsWith("MP-PROVIDER-004")) {
    return "The provider needs you to sign in inside the Movie Party browser before playback.";
  }
  if (code.startsWith("MP-PROVIDER-")) {
    return "The provider page is not ready for playback yet. Open a title, then try again.";
  }
  if (code.startsWith("MP-SYNC-")) {
    return "Playback could not stay synchronized. Movie Party paused both sides to stay together.";
  }
  if (code.startsWith("MP-CALL-")) {
    return "The call could not be established. The movie continues.";
  }
  if (code.startsWith("MP-STORE-")) {
    return "Movie Party could not reach its local storage. Retry, or restart the app.";
  }
  if (code.startsWith("MP-MEDIA-")) {
    return "The movie file could not be opened. Check that it still exists on disk.";
  }
  if (code.startsWith("MP-MP-") || code.startsWith("MP-BACKEND")) {
    return "Movie Party couldn't continue this session.";
  }
  return "Movie Party couldn't continue this session.";
}

export function ErrorScreenView({
  code,
  message,
  actions,
  technicalDetails,
  onDismiss,
}: ErrorScreenViewProps) {
  const [detailsOpen, setDetailsOpen] = useState(false);

  // Keyboard: Escape dismisses when the caller allows it.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && onDismiss) {
        onDismiss();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [onDismiss]);

  return (
    <div
      className="relative w-screen h-screen overflow-hidden"
      data-testid="error-screen"
      role="alert"
    >
      <SilkBackground variant="calm" />
      <main className="relative z-10 h-full flex items-center justify-center px-12">
        <section className="max-w-xl w-full" aria-labelledby="error-heading">
          <div className="flex items-center gap-3 text-white/50">
            <AlertTriangle className="w-5 h-5 text-amber-300/90" strokeWidth={1.6} />
            <span className="text-[11px] tracking-[0.28em] uppercase">
              Session interrupted
            </span>
          </div>

          <h1
            id="error-heading"
            className="font-serif-display text-white text-[38px] leading-[1.05] tracking-[-0.02em] mt-5"
          >
            {message}
          </h1>

          {code ? (
            <p
              className="mt-4 text-sm text-white/70 font-mono-mp"
              data-testid="error-code"
            >
              {code}
            </p>
          ) : null}

          <div className="mt-10 flex flex-wrap items-center gap-4">
            {actions.map((action) => (
              <CinemaButton
                key={action.label}
                variant={action.variant ?? "ghost"}
                onClick={action.onClick}
              >
                {action.label}
              </CinemaButton>
            ))}
          </div>

          {technicalDetails ? (
            <div className="mt-10">
              <button
                type="button"
                onClick={() => {
      setDetailsOpen((open) => !open);
    }}
                aria-expanded={detailsOpen}
                className="flex items-center gap-2 text-[11px] tracking-[0.22em] uppercase text-white/40 hover:text-white/70 transition"
              >
                <ChevronDown
                  className={`w-3.5 h-3.5 transition-transform ${
                    detailsOpen ? "rotate-180" : ""
                  }`}
                  strokeWidth={1.6}
                />
                Technical details
              </button>
              {detailsOpen ? (
                <pre className="mt-3 p-4 rounded-xl bg-white/[0.04] border border-white/10 text-xs text-white/60 font-mono-mp whitespace-pre-wrap break-words max-h-48 overflow-y-auto">
                  {technicalDetails}
                </pre>
              ) : null}
            </div>
          ) : null}

          {onDismiss ? (
            <button
              type="button"
              onClick={onDismiss}
              className="mt-10 inline-flex items-center gap-2 text-sm text-white/50 hover:text-white transition"
            >
              <RotateCcw className="w-4 h-4" strokeWidth={1.6} />
              Back to lobby
            </button>
          ) : null}
        </section>
      </main>
    </div>
  );
}
