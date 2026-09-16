import { ArrowLeft, ExternalLink, RefreshCw, UsersRound } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";

type PartnerConnectViewProps = {
  isRetrying: boolean;
  onRetry: () => void;
  onOpenHelp: () => void;
  /**
   * Escape hatch (F4). This screen replaces the whole app when a join fails
   * with MP-NET-TS-005, so without a way back the only exit was quitting.
   * Wired to AppShell's canonical goHome path, which clears the stale join
   * error, failure code and pending invite.
   */
  onCancel: () => void;
};

export function PartnerConnectView({
  isRetrying,
  onRetry,
  onOpenHelp,
  onCancel,
}: PartnerConnectViewProps) {
  return (
    <div className="relative min-h-screen overflow-hidden bg-[#05050B] text-white">
      <SilkBackground variant="dim" />
      <main className="relative z-10 mx-auto flex min-h-screen max-w-5xl items-center px-6 py-12 sm:px-12">
        <section className="grid max-w-2xl gap-7">
          <button
            type="button"
            onClick={onCancel}
            className="flex items-center gap-2 text-white/60 hover:text-white transition text-sm tracking-wider w-fit"
            data-testid="partner-connect-back-btn"
          >
            <ArrowLeft className="w-4 h-4" strokeWidth={1.6} /> Back
          </button>
          <div className="flex items-center gap-3 text-white/65">
            <span className="flex h-11 w-11 items-center justify-center rounded-full border border-[#9F7AEA]/35 bg-[#6B46C1]/20">
              <UsersRound className="h-5 w-5 text-[#C4B5FD]" strokeWidth={1.6} />
            </span>
            <span className="text-[11px] uppercase tracking-[0.28em]">Private connection</span>
          </div>
          <h1 className="font-serif-display text-5xl leading-[0.95] sm:text-6xl">
            Connect to your
            <br />
            <span className="italic">movie partner.</span>
          </h1>
          <p className="max-w-xl text-base leading-relaxed text-white/60">
            Both devices use Tailscale, but this device cannot currently reach the host.
          </p>
          <ul className="grid gap-2 text-sm leading-relaxed text-white/55">
            <li>Make sure the host device is online.</li>
            <li>Make sure both devices are allowed to communicate through Tailscale.</li>
            <li>Share or invite the guest in Tailscale when the devices use separate tailnets.</li>
          </ul>
          <div className="flex flex-wrap items-center gap-3 pt-2">
            <CinemaButton icon={RefreshCw} onClick={onRetry} disabled={isRetrying}>
              Try Again
            </CinemaButton>
            <button
              type="button"
              onClick={onOpenHelp}
              className="inline-flex h-12 items-center gap-2 px-4 text-sm tracking-wide text-white/70 transition hover:text-white"
            >
              <ExternalLink className="h-4 w-4" />
              Setup help
            </button>
            <button
              type="button"
              onClick={onCancel}
              className="inline-flex h-12 items-center gap-2 px-4 text-sm tracking-wide text-white/70 transition hover:text-white"
              data-testid="partner-connect-cancel-btn"
            >
              Cancel and go home
            </button>
          </div>
        </section>
      </main>
    </div>
  );
}
