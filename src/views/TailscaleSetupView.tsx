import {
  AlertTriangle,
  Download,
  ExternalLink,
  LogIn,
  Power,
  RefreshCw,
  ShieldCheck,
  Wifi,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import type {
  TailscaleReadiness,
  TailscaleState,
  TailscaleSetupAction,
} from "../backend/appRuntime";

type TailscaleSetupViewProps = {
  readiness: TailscaleReadiness;
  isRefreshing: boolean;
  onRefresh: () => void;
  onOpenSetup: (action: TailscaleSetupAction) => void;
};

export type StateContent = {
  eyebrow: string;
  title: string;
  description: string;
  primaryLabel: string;
  primaryAction: TailscaleSetupAction;
  icon: LucideIcon;
  accent: string;
};

export function contentFor(state: TailscaleState): StateContent {
  switch (state) {
    case "NOT_INSTALLED":
      return {
        eyebrow: "Private connection setup",
        title: "Set up your\nprivate connection",
        description:
          "Movie Party connects two devices privately through Tailscale. Tailscale isn't installed on this device yet. Install it, sign in, and come back to start your movie night.",
        primaryLabel: "Install Tailscale",
        primaryAction: "INSTALL",
        icon: Download,
        accent: "text-[#C4B5FD] border-[#9F7AEA]/35 bg-[#6B46C1]/20",
      };
    case "DAEMON_UNAVAILABLE":
      return {
        eyebrow: "Private connection setup",
        title: "Start Tailscale\non this device",
        description:
          "Tailscale is installed on this device, but it isn't responding right now. Open the Tailscale app to start it, then check again.",
        primaryLabel: "Open Tailscale",
        primaryAction: "OPEN_APP",
        icon: Power,
        accent: "text-[#FCD34D] border-[#FBBF24]/35 bg-[#B45309]/20",
      };
    case "NEEDS_LOGIN":
      return {
        eyebrow: "Private connection setup",
        title: "Sign in to\nTailscale",
        description:
          "Tailscale is installed, but this device isn't signed in to a Tailscale network yet. Open the Tailscale app and sign in to your account, then come back.",
        primaryLabel: "Open Tailscale",
        primaryAction: "OPEN_APP",
        icon: LogIn,
        accent: "text-[#C4B5FD] border-[#9F7AEA]/35 bg-[#6B46C1]/20",
      };
    case "STOPPED":
      return {
        eyebrow: "Private connection setup",
        title: "Turn on\nTailscale",
        description:
          "Tailscale is installed and set up, but the connection is currently off. Open the Tailscale app and turn it on, then check again.",
        primaryLabel: "Open Tailscale",
        primaryAction: "OPEN_APP",
        icon: Wifi,
        accent: "text-[#FCD34D] border-[#FBBF24]/35 bg-[#B45309]/20",
      };
    case "NO_USABLE_ADDRESS":
      return {
        eyebrow: "Private connection setup",
        title: "Connect to your\nprivate network",
        description:
          "Tailscale is running, but this device isn't connected to a private address that Movie Party can use. Open the Tailscale app and make sure it's connected, then check again.",
        primaryLabel: "Open Tailscale",
        primaryAction: "OPEN_APP",
        icon: AlertTriangle,
        accent: "text-[#FCA5A5] border-[#F87171]/35 bg-[#7F1D1D]/20",
      };
    default:
      return {
        eyebrow: "Private connection setup",
        title: "Check your\nprivate connection",
        description:
          "Movie Party uses Tailscale to securely connect the two devices. Finish setting up Tailscale, then check again.",
        primaryLabel: "Open Tailscale",
        primaryAction: "OPEN_APP",
        icon: ShieldCheck,
        accent: "text-[#C4B5FD] border-[#9F7AEA]/35 bg-[#6B46C1]/20",
      };
  }
}

export function TailscaleSetupView({
  readiness,
  isRefreshing,
  onRefresh,
  onOpenSetup,
}: TailscaleSetupViewProps) {
  const content = contentFor(readiness.state);
  const StateIcon = content.icon;
  const refreshLabel = readiness.state === "NEEDS_LOGIN" ? "I've signed in" : "Check again";

  return (
    <div className="relative min-h-screen w-full overflow-hidden bg-[#05050B] text-white">
      <SilkBackground />
      <main className="relative z-10 flex min-h-screen w-full flex-col items-center justify-center px-6 py-16 sm:px-12">
        <section className="flex w-full max-w-2xl flex-col items-center text-center">
          <div
            className={`flex h-14 w-14 items-center justify-center rounded-full border ${content.accent}`}
          >
            <StateIcon className="h-6 w-6" strokeWidth={1.6} />
          </div>

          <span className="mt-6 text-[11px] uppercase tracking-[0.28em] text-white/60">
            {content.eyebrow}
          </span>

          <h1 className="mt-4 whitespace-pre-line font-serif-display text-5xl leading-[0.95] sm:text-6xl">
            {content.title}
          </h1>

          <p className="mt-6 max-w-xl text-base leading-relaxed text-white/60 sm:text-lg">
            {content.description}
          </p>

          {readiness.message && readiness.message !== content.description && (
            <p className="mt-4 max-w-xl text-sm leading-relaxed text-white/45">
              {readiness.message}
            </p>
          )}

          <div className="mt-10 flex flex-wrap items-center justify-center gap-3">
            <CinemaButton
              icon={ExternalLink}
              onClick={() => {
                onOpenSetup(content.primaryAction);
              }}
            >
              {content.primaryLabel}
            </CinemaButton>
            <button
              type="button"
              onClick={onRefresh}
              disabled={isRefreshing}
              className="inline-flex h-14 items-center gap-2 rounded-full px-6 text-sm tracking-wide text-white/70 transition hover:text-white disabled:opacity-45"
            >
              <RefreshCw className={`h-4 w-4 ${isRefreshing ? "animate-spin" : ""}`} />
              {refreshLabel}
            </button>
          </div>

          <p className="mt-8 text-xs leading-relaxed text-white/40">
            Movie Party never asks for your Tailscale account or password.
          </p>
        </section>
      </main>
    </div>
  );
}
