import { useEffect, useState } from "react";
import { Check, Copy, QrCode, X } from "lucide-react";
import { inviteQrSvg } from "../../backend/appRuntime";

/**
 * §16 INVITATION — the host's full invite surface: copy the short code,
 * copy the full movieparty:// link (browser/OS-openable — the deep-link
 * plugin hands it to the app), and show a scannable QR for moving the
 * invite across devices.
 *
 * The invite link stays a link: pasting it into any browser on a machine
 * with Movie Party installed opens the join screen with the ticket
 * pre-filled (AppShell consumes deep links + startup args).
 */
export function InviteCard({
  inviteCode,
  title,
  qrHint,
}: {
  inviteCode: string;
  /** Optional label overrides (the Friends surface uses its own copy). */
  title?: string;
  qrHint?: string;
}) {
  const [mode, setMode] = useState<"code" | "qr">("code");
  const [qr, setQr] = useState<string | null>(null);
  const [copied, setCopied] = useState<"none" | "code" | "link">("none");

  // The full movieparty:// URL is what the OS/browser opens; the code
  // alone is for humans comparing screens.
  const isFullLink = inviteCode.startsWith("movieparty://");

  useEffect(() => {
    if (mode !== "qr" || qr != null || !isFullLink) {
      return;
    }
    let cancelled = false;
    void inviteQrSvg(inviteCode).then((svg) => {
      if (!cancelled) {
        setQr(svg);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [mode, qr, inviteCode, isFullLink]);

  const copy = async (what: "code" | "link") => {
    try {
      await navigator.clipboard.writeText(inviteCode);
      setCopied(what);
      window.setTimeout(() => {
        setCopied("none");
      }, 1600);
    } catch {
      /* clipboard unavailable — the code stays visible to read */
    }
  };

  return (
    <div
      className="rounded-2xl p-5 min-w-0 max-w-full"
      style={{
        background: "rgba(13, 11, 20, 0.7)",
        border: "1px solid rgba(159,122,234,0.18)",
        backdropFilter: "blur(14px)",
      }}
      data-testid="invite-card"
    >
      <div className="flex items-center justify-between">
        <span className="text-[11px] tracking-[0.28em] uppercase text-white/50">
          {mode === "qr" ? (qrHint ?? "Scan to join") : (title ?? "Invite link")}
        </span>
        <button
          type="button"
          onClick={() => {
            setMode(mode === "qr" ? "code" : "qr");
          }}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-white/5 hover:bg-white/10 border border-white/10 transition text-[10px] tracking-widest uppercase text-white/70"
          data-testid="invite-qr-toggle"
        >
          {mode === "qr" ? <X className="w-3 h-3" /> : <QrCode className="w-3 h-3" />}
          {mode === "qr" ? "Close QR" : "Show QR"}
        </button>
      </div>

      {mode === "code" ? (
        <div className="mt-3 flex items-center justify-between gap-3 min-w-0">
          <span
            className="min-w-0 flex-1 truncate font-mono-mp text-white text-sm tracking-[0.12em]"
            data-testid="invite-value"
            title={inviteCode}
          >
            {inviteCode}
          </span>
          <button
            type="button"
            onClick={() => void copy("code")}
            className="flex items-center gap-2 px-4 py-2 rounded-full bg-white/5 hover:bg-white/10 border border-white/10 transition text-xs tracking-widest uppercase text-white/80"
            data-testid="invite-copy-btn"
          >
            {copied !== "none" ? (
              <Check className="w-3.5 h-3.5 text-[#34D399]" />
            ) : (
              <Copy className="w-3.5 h-3.5" />
            )}
            {copied !== "none" ? "Copied" : isFullLink ? "Copy link" : "Copy invite"}
          </button>
        </div>
      ) : (
        <div className="mt-3 flex flex-col items-center gap-2.5">
          <div className="rounded-xl bg-white p-3" data-testid="invite-qr">
            {qr == null ? (
              <div className="w-40 h-40 flex items-center justify-center">
                {isFullLink ? (
                  <span className="text-xs text-black/50">Rendering…</span>
                ) : (
                  <span className="text-xs text-black/60 text-center px-4">
                    QR needs the full invite link
                  </span>
                )}
              </div>
            ) : (
              <div
                className="w-40 h-40 invite-qr-box"
                // The SVG is generated locally from the invite link by our
                // own Rust command (invite_qr_svg) — trusted content, no
                // remote origin. SVG injection via innerHTML is scoped to
                // this renderer output.
                dangerouslySetInnerHTML={{ __html: qr }}
              />
            )}
          </div>
          <p className="text-[10px] text-white/40 text-center leading-relaxed max-w-[260px]">
            Scan with the other device's camera — it opens Movie Party straight
            to the join screen with the invite filled in.
          </p>
          {isFullLink ? (
            <button
              type="button"
              onClick={() => void copy("link")}
              className="flex items-center gap-2 px-4 py-2 rounded-full bg-white/5 hover:bg-white/10 border border-white/10 transition text-xs tracking-widest uppercase text-white/80"
              data-testid="invite-copy-link-btn"
            >
              {copied === "link" ? (
                <Check className="w-3.5 h-3.5 text-[#34D399]" />
              ) : (
                <Copy className="w-3.5 h-3.5" />
              )}
              {copied === "link" ? "Copied link" : "Copy movieparty:// link"}
            </button>
          ) : null}
        </div>
      )}
    </div>
  );
}
