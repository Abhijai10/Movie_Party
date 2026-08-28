import { ExternalLink, RefreshCw, ShieldCheck } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import type { TailscaleReadiness } from "../backend/appRuntime";

type TailscaleSetupViewProps = {
  readiness: TailscaleReadiness;
  isRefreshing: boolean;
  onRefresh: () => void;
  onOpenSetup: (action: "INSTALL" | "SIGN_IN") => void;
};

export function TailscaleSetupView({
  readiness,
  isRefreshing,
  onRefresh,
  onOpenSetup,
}: TailscaleSetupViewProps) {
  const signedOut = readiness.state === "SIGNED_OUT";
  const unavailable = readiness.state === "UNAVAILABLE";
  const title = signedOut
    ? "Sign in to\nTailscale."
    : unavailable
      ? "Check your\nprivate connection."
      : "Private connection\nsetup.";
  const primaryLabel = signedOut ? "Sign in to Tailscale" : "Set up Tailscale";

  return (
    <div className="relative min-h-screen overflow-hidden bg-[#05050B] text-white">
      <SilkBackground />
      <main className="relative z-10 mx-auto flex min-h-screen max-w-5xl items-center px-6 py-12 sm:px-12">
        <section className="grid max-w-2xl gap-7">
          <div className="flex items-center gap-3 text-white/65">
            <span className="flex h-11 w-11 items-center justify-center rounded-full border border-[#9F7AEA]/35 bg-[#6B46C1]/20">
              <ShieldCheck className="h-5 w-5 text-[#C4B5FD]" strokeWidth={1.6} />
            </span>
            <span className="text-[11px] uppercase tracking-[0.28em]">Private connection setup</span>
          </div>
          <h1 className="whitespace-pre-line font-serif-display text-5xl leading-[0.95] sm:text-6xl">
            {title}
          </h1>
          <p className="max-w-xl text-base leading-relaxed text-white/60">
            Movie Party uses Tailscale to securely connect the two devices. {readiness.message}
          </p>
          <div className="flex flex-wrap items-center gap-3 pt-2">
            <CinemaButton
              icon={ExternalLink}
              onClick={() => {
                onOpenSetup(signedOut ? "SIGN_IN" : "INSTALL");
              }}
            >
              {primaryLabel}
            </CinemaButton>
            <button
              type="button"
              onClick={onRefresh}
              disabled={isRefreshing}
              className="inline-flex h-12 items-center gap-2 px-4 text-sm tracking-wide text-white/70 transition hover:text-white disabled:opacity-45"
            >
              <RefreshCw className={`h-4 w-4 ${isRefreshing ? "animate-spin" : ""}`} />
              {signedOut ? "I've signed in" : "Check again"}
            </button>
          </div>
          <p className="text-xs leading-relaxed text-white/40">
            Movie Party never asks for your Tailscale account or password.
          </p>
        </section>
      </main>
    </div>
  );
}
