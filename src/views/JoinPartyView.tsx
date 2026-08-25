import { motion } from "framer-motion";
import { ArrowLeft, ArrowRight, Check, ClipboardPaste, Copy } from "lucide-react";
import { useEffect, useState } from "react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { MovieTicket } from "../components/mp/MovieTicket";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import { previewInviteRoomCode } from "../invites/deepLinks";

type JoinPartyViewProps = {
  isJoining: boolean;
  error: string | null;
  initialInvite?: string;
  onBack: () => void;
  onJoin: (inviteCode: string) => Promise<boolean>;
};

export function JoinPartyView({
  isJoining,
  error,
  initialInvite = "",
  onBack,
  onJoin,
}: JoinPartyViewProps) {
  const [code, setCode] = useState("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (initialInvite) {
      setCode(initialInvite);
    }
  }, [initialInvite]);

  const paste = async () => {
    try {
      const text = await navigator.clipboard.readText();
      setCode(text.trim());
    } catch {
      /* clipboard unavailable */
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => {
        setCopied(false);
      }, 1500);
    } catch {
      /* clipboard unavailable */
    }
  };

  const join = () => {
    if (!code.trim()) return;
    void onJoin(code.trim());
  };

  const displayCode = previewInviteRoomCode(code);

  return (
    <div className="relative w-screen h-screen overflow-hidden">
      <SilkBackground variant="warm" />

      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <button
          type="button"
          onClick={onBack}
          className="flex items-center gap-2 text-white/60 hover:text-white transition text-sm tracking-wider"
          data-testid="join-back-btn"
        >
          <ArrowLeft className="w-4 h-4" strokeWidth={1.6} /> Back
        </button>
        <span className="text-[11px] tracking-[0.28em] uppercase text-white/50">
          Guest entrance
        </span>
      </header>

      <main className="relative z-10 max-w-[1300px] mx-auto px-12 mt-6 grid grid-cols-12 gap-12 items-center h-[calc(100vh-120px)]">
        <motion.section
          initial={{ opacity: 0, x: -30 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 1, ease: [0.22, 1, 0.36, 1] }}
          className="col-span-12 lg:col-span-6 flex items-center justify-center"
        >
          <MovieTicket code={displayCode} />
        </motion.section>

        <motion.section
          initial={{ opacity: 0, x: 30 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 1, ease: [0.22, 1, 0.36, 1], delay: 0.1 }}
          className="col-span-12 lg:col-span-6"
        >
          <span className="text-[11px] tracking-[0.32em] uppercase text-white/50">
            ● You've been invited
          </span>
          <h1 className="font-serif-display text-white text-[64px] leading-[0.94] tracking-tight mt-5">
            Your friend
            <br />
            <span className="italic">invited you.</span>
          </h1>
          <p className="mt-6 text-white/55 text-base leading-relaxed max-w-md">
            Enter the private cinema when the room is ready for you. Two seats, one screen,
            perfectly synchronized.
          </p>

          <div className="mt-10 max-w-md">
            <label className="text-[11px] tracking-[0.24em] uppercase text-white/45">
              Move Party invite link
            </label>

            <div className="mt-3 relative flex items-center rounded-xl border border-white/10 bg-white/[0.03] focus-within:border-[#9F7AEA]/50 transition">
              <input
                type="text"
                value={code}
                onChange={(e) => {
                  setCode(e.target.value);
                }}
                placeholder="moveparty://join/..."
                maxLength={4096}
                className="min-w-0 flex-1 bg-transparent px-4 py-3.5 text-white text-sm tracking-[0.04em] font-mono-mp placeholder:text-white/25 focus:outline-none"
                data-testid="join-code-input"
              />
              <button
                type="button"
                onClick={() => void paste()}
                className="px-3 py-2 text-white/70 hover:text-white text-xs tracking-widest uppercase flex items-center gap-1.5 border-l border-white/10"
                data-testid="join-paste-btn"
              >
                <ClipboardPaste className="w-3.5 h-3.5" /> Paste
              </button>
              <button
                type="button"
                onClick={() => void copy()}
                disabled={!code}
                className="px-3 py-2 text-white/70 hover:text-white text-xs tracking-widest uppercase flex items-center gap-1.5 border-l border-white/10 disabled:opacity-30"
                data-testid="join-copy-btn"
              >
                {copied ? (
                  <Check className="w-3.5 h-3.5 text-[#34D399]" />
                ) : (
                  <Copy className="w-3.5 h-3.5" />
                )}
                {copied ? "Copied" : "Copy"}
              </button>
            </div>

            <div className="mt-4">
              <StatusIndicator
                state="waiting"
                label={
                  isJoining ? "Opening a secure path to the room..." : "Waiting for your invite"
                }
              />
            </div>

            {error ? (
              <div
                className="mt-4 flex items-center gap-2 text-[#F87171] text-xs tracking-wider"
                role="alert"
                data-testid="join-error"
              >
                {error}
              </div>
            ) : null}

            <div className="mt-8">
              <CinemaButton
                onClick={join}
                icon={ArrowRight}
                disabled={!code.trim() || isJoining}
                data-testid="join-cinema-btn"
              >
                Join Cinema
              </CinemaButton>
            </div>
          </div>
        </motion.section>
      </main>
    </div>
  );
}
