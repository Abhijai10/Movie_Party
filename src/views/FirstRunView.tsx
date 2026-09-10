import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Check, CircleDashed, X } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { getPrerequisiteStatuses } from "../backend/appRuntime";
import type { PrerequisiteStatus } from "../backend/appRuntime";

export type { PrerequisiteStatus };

/**
 * UI_UX_SPEC §10 + §11 — First Run welcome + prerequisite checks.
 *
 * Truth rules (§38 + §11):
 * - every row shows the REAL detected state — "Not requested" for
 *   permissions we have not asked for yet. NEVER a green check for an
 *   unverified thing, and NEVER a premature permission prompt (the §11
 *   rule: do not request camera/screen before necessary unless the
 *   onboarding explains why);
 * - missing prerequisites are surfaced honestly with what breaks without
 *   them — no silent fallback (§29);
 * - Continue is always available: nothing here blocks entering the app
 *   (Netflix sync needs Chrome; local playback needs libmpv; the missing
 *   piece disables exactly its own capability).
 */
type FirstRunViewProps = {
  onContinue: () => void;
};

function stateIcon(state: PrerequisiteStatus["state"]) {
  if (state === "OK") {
    return <Check className="w-4 h-4 text-emerald-300" strokeWidth={2.2} />;
  }
  if (state === "MISSING") {
    return <X className="w-4 h-4 text-rose-300" strokeWidth={2.2} />;
  }
  if (state === "NOT_REQUESTED") {
    return <CircleDashed className="w-4 h-4 text-white/35" strokeWidth={1.6} />;
  }
  return <CircleDashed className="w-4 h-4 text-sky-300/80" strokeWidth={1.6} />;
}

export function stateLabelFor(state: PrerequisiteStatus["state"]): string {
  switch (state) {
    case "OK":
      return "Ready";
    case "MISSING":
      return "Not found";
    case "NOT_REQUESTED":
      return "Not requested";
    case "OPTIONAL":
      return "Optional";
  }
}

export function FirstRunView({ onContinue }: FirstRunViewProps) {
  const [checks, setChecks] = useState<PrerequisiteStatus[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getPrerequisiteStatuses().then((statuses) => {
      if (!cancelled) {
        setChecks(statuses);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="relative w-screen h-screen overflow-hidden" data-testid="first-run">
      <SilkBackground />
      <main className="relative z-10 h-full flex items-center justify-center px-12">
        <motion.section
          initial={{ opacity: 0, y: 18 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, ease: [0.22, 1, 0.36, 1] }}
          className="max-w-lg w-full"
        >
          <span className="text-[11px] tracking-[0.32em] uppercase text-white/50">
            Welcome to Movie Party
          </span>
          <h1 className="font-serif-display text-white text-[52px] leading-[0.98] tracking-[-0.02em] mt-4">
            Watch together.
            <br />
            <span className="italic text-white/90">Stay synchronized.</span>
          </h1>

          <div className="mt-12">
            <h2 className="text-[11px] tracking-[0.28em] uppercase text-white/50 mb-4">
              Setup
            </h2>
            {checks === null ? (
              <p className="text-sm text-white/50" data-testid="first-run-checks-loading">
                Checking this Mac…
              </p>
            ) : (
              <ul className="space-y-3" data-testid="first-run-checks">
                {checks.map((check) => (
                  <li
                    key={check.id}
                    className="flex items-start justify-between gap-6"
                    data-testid={`prereq-${check.id}`}
                  >
                    <span className="flex items-baseline gap-3 min-w-0">
                      <span className="flex items-center gap-2.5 text-sm text-white/85">
                        {stateIcon(check.state)}
                        {check.label}
                      </span>
                      <span className="text-xs text-white/45 truncate">{check.detail}</span>
                    </span>
                    <span
                      className={`shrink-0 text-[11px] tracking-[0.14em] uppercase ${
                        check.state === "MISSING"
                          ? "text-rose-300/90"
                          : check.state === "OK"
                            ? "text-emerald-300/80"
                            : "text-white/40"
                      }`}
                    >
                      {stateLabelFor(check.state)}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>

          <div className="mt-12">
            <CinemaButton onClick={onContinue} data-testid="first-run-continue">
              Get Started
            </CinemaButton>
          </div>
        </motion.section>
      </main>
    </div>
  );
}
