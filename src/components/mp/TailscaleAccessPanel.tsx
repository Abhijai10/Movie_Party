import { useState, type JSX, type SyntheticEvent } from "react";
import {
  ExternalLink,
  Eye,
  Loader2,
  MinusCircle,
  ShieldCheck,
  UserMinus,
  UsersRound,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { CinemaButton } from "./CinemaButton";
import type {
  AccessMatrixView,
  DestructiveActionId,
  RevocationView,
  SharePlanView,
} from "../../backend/appRuntime";
import {
  ACTION_CARDS,
  actionCard,
  capabilityLine,
  matrixSummary,
  revocationLine,
  revocationTone,
  sharePlanBlocker,
  sharePlanLines,
} from "../../onboarding/access";

/**
 * Batch 10's access surface.
 *
 * It performs no Tailscale write. Two of the three actions live in Tailscale's
 * console because the documented API has no scope for them; the third is local
 * to Movie Party. Each has its own confirmation, and each says plainly what it
 * is *not* — that separation is the point of the panel.
 */
export type TailscaleAccessPanelProps = {
  matrix: AccessMatrixView | null;
  plan: SharePlanView | null;
  revocation: RevocationView | null;
  busy: string | null;
  error: string | null;
  onPlan: (recipient: string, lifetimeDays: number) => void;
  onOpenConsole: (action: DestructiveActionId) => void;
  onCheckRevocation: () => void;
  /** The existing, local friendship removal. Never a Tailscale action. */
  onRemoveFriend: () => void;
};

const CARD = "rounded-xl border border-white/10 bg-white/[0.03] p-4";
const EYEBROW = "text-[10px] uppercase tracking-[0.24em] text-white/45";

const ACTION_ICON: Record<DestructiveActionId, LucideIcon> = {
  REMOVE_MOVIE_PARTY_FRIEND: UserMinus,
  REVOKE_TAILSCALE_DEVICE_SHARE: MinusCircle,
  REMOVE_TAILNET_MEMBER: UsersRound,
};

export function TailscaleAccessPanel({
  matrix,
  plan,
  revocation,
  busy,
  error,
  onPlan,
  onOpenConsole,
  onCheckRevocation,
  onRemoveFriend,
}: TailscaleAccessPanelProps): JSX.Element {
  const [recipient, setRecipient] = useState("");
  const [lifetime, setLifetime] = useState("7");
  const [confirming, setConfirming] = useState<DestructiveActionId | null>(null);

  const submit = (event: SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();
    const days = Number(lifetime);
    if (!Number.isFinite(days)) return;
    onPlan(recipient.trim(), Math.round(days));
  };

  const blocker = plan ? sharePlanBlocker(plan) : null;

  return (
    <section className={CARD} data-testid="onboarding-access-panel">
      <span className={EYEBROW}>Tailscale access</span>
      <p className="mt-2 text-sm leading-relaxed text-white/60" data-testid="onboarding-access-summary">
        {matrix ? matrixSummary(matrix) : "Reading what Movie Party may automate…"}
      </p>

      {error != null ? (
        <p role="alert" className="mt-3 text-sm text-[#FCA5A5]" data-testid="onboarding-access-error">
          {error}
        </p>
      ) : null}

      {/* Share plan: a plan, never a write. */}
      <div className="mt-4 rounded-lg border border-white/10 p-3" data-testid="onboarding-share-plan">
        <span className={EYEBROW}>Share this machine</span>
        <p className="mt-2 text-xs leading-relaxed text-white/55">
          One machine, one person, a bounded lifetime. Movie Party prepares the plan and opens
          Tailscale&apos;s page — it never creates the share itself.
        </p>
        <form className="mt-3 flex flex-wrap items-end gap-3" onSubmit={submit}>
          <label className="flex flex-col gap-1 text-xs text-white/60">
            Their Tailscale account
            <input
              className="mp-input font-mono-mp"
              type="email"
              autoComplete="off"
              spellCheck={false}
              value={recipient}
              onChange={(event) => {
                setRecipient(event.target.value);
              }}
              data-testid="onboarding-share-recipient"
              placeholder="them@example.com"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-white/60">
            Days (max 30)
            <input
              className="mp-input font-mono-mp w-24"
              type="number"
              min={1}
              max={30}
              value={lifetime}
              onChange={(event) => {
                setLifetime(event.target.value);
              }}
              data-testid="onboarding-share-lifetime"
            />
          </label>
          <CinemaButton
            type="submit"
            variant="neutral"
            disabled={busy === "plan" || recipient.trim().length === 0}
            data-testid="onboarding-share-plan-submit"
          >
            {busy === "plan" ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
            Prepare the plan
          </CinemaButton>
        </form>

        {plan ? (
          <div className="mt-3 space-y-1.5" data-testid="onboarding-share-plan-result">
            {sharePlanLines(plan).map((line) => (
              <p key={line} className="text-xs leading-relaxed text-white/60">
                {line}
              </p>
            ))}
            {blocker != null ? (
              <p className="text-xs text-amber-200" data-testid="onboarding-share-plan-blocker">
                {blocker}
              </p>
            ) : null}
            <CinemaButton
              variant="ghost"
              disabled={blocker != null}
              onClick={() => {
                setConfirming("REVOKE_TAILSCALE_DEVICE_SHARE");
              }}
              data-testid="onboarding-share-open-console"
            >
              <ExternalLink className="w-4 h-4" /> Open Tailscale&apos;s Machines page
            </CinemaButton>
          </div>
        ) : null}
      </div>

      {/* Revocation state: observed, never asserted as an action taken. */}
      <div className="mt-4 rounded-lg border border-white/10 p-3" data-testid="onboarding-revocation">
        <span className={EYEBROW}>Is the share still live?</span>
        <p
          className={`mt-2 text-xs leading-relaxed ${
            revocation ? revocationTone(revocation) === "ok" ? "text-emerald-200" : "text-white/60" : "text-white/45"
          }`}
          data-testid="onboarding-revocation-state"
        >
          {revocation ? revocationLine(revocation) : "Not checked yet."}
        </p>
        <div className="mt-3 flex flex-wrap gap-3">
          <CinemaButton
            variant="ghost"
            disabled={busy === "revocation"}
            onClick={onCheckRevocation}
            data-testid="onboarding-revocation-check"
          >
            {busy === "revocation" ? <Loader2 className="w-4 h-4 animate-spin" /> : <Eye className="w-4 h-4" />}
            Check again
          </CinemaButton>
          <CinemaButton
            variant="neutral"
            onClick={() => {
              setConfirming("REVOKE_TAILSCALE_DEVICE_SHARE");
            }}
            data-testid="onboarding-revocation-console"
          >
            <ExternalLink className="w-4 h-4" /> Revoke in Tailscale
          </CinemaButton>
        </div>
      </div>

      {/* Three separate actions, three separate confirmations. */}
      <div className="mt-4 grid gap-3 sm:grid-cols-3" data-testid="onboarding-removal-actions">
        {ACTION_CARDS.map((card) => {
          const Icon = ACTION_ICON[card.id];
          const isConfirming = confirming === card.id;
          return (
            <div
              key={card.id}
              className="rounded-lg border border-white/10 p-3 flex flex-col gap-2"
              data-testid={`onboarding-action-${card.id}`}
            >
              <span className="flex items-center gap-2 text-xs text-white/80">
                <Icon className="w-3.5 h-3.5 shrink-0" strokeWidth={1.6} />
                {card.title}
              </span>
              <p className="text-[11px] leading-relaxed text-white/55">{card.effect}</p>
              <p className="text-[11px] leading-relaxed text-white/40">{card.notThis}</p>
              {isConfirming ? (
                <div className="mt-1 space-y-2" data-testid={`onboarding-action-confirm-${card.id}`}>
                  <p className="text-[11px] leading-relaxed text-amber-100/90">{card.confirmBody}</p>
                  <div className="flex flex-wrap gap-2">
                    <CinemaButton
                      variant="primary"
                      className="!min-h-10 !px-4"
                      onClick={() => {
                        setConfirming(null);
                        if (card.needsConsole) {
                          onOpenConsole(card.id);
                        } else {
                          onRemoveFriend();
                        }
                      }}
                      data-testid={`onboarding-action-do-${card.id}`}
                    >
                      {card.needsConsole ? "Open Tailscale" : "Remove friend"}
                    </CinemaButton>
                    <CinemaButton
                      variant="ghost"
                      className="!min-h-10 !px-4"
                      onClick={() => {
                        setConfirming(null);
                      }}
                      data-testid={`onboarding-action-cancel-${card.id}`}
                    >
                      Cancel
                    </CinemaButton>
                  </div>
                </div>
              ) : (
                <CinemaButton
                  variant="ghost"
                  className="!min-h-10 !px-4 mt-auto"
                  onClick={() => {
                    setConfirming(card.id);
                  }}
                  data-testid={`onboarding-action-start-${card.id}`}
                >
                  {card.confirmTitle}
                </CinemaButton>
              )}
              <p className="text-[10px] uppercase tracking-[0.18em] text-white/35">
                {card.performedBy}
              </p>
            </div>
          );
        })}
      </div>

      {matrix ? (
        <details className="mt-4" data-testid="onboarding-access-matrix">
          <summary className="cursor-pointer text-[11px] uppercase tracking-[0.2em] text-white/45">
            What is documented, and what is not
          </summary>
          <ul className="mt-3 space-y-2">
            {matrix.rows.map((row) => (
              <li key={row.operation} className="text-[11px] leading-relaxed text-white/55">
                <span className="text-white/80">{row.operation}</span> — {capabilityLine(row)}
                <a
                  className="ml-1 underline decoration-white/20 underline-offset-2 hover:text-white/80"
                  href={row.evidenceUrl}
                  target="_blank"
                  rel="noreferrer noopener"
                >
                  source
                </a>
              </li>
            ))}
          </ul>
          <p className="mt-3 flex items-start gap-2 text-[11px] leading-relaxed text-white/45">
            <ShieldCheck className="w-3.5 h-3.5 shrink-0 mt-0.5" strokeWidth={1.6} />
            {matrix.sharedWithExternalUserNote}
          </p>
          <p className="mt-2 text-[11px] leading-relaxed text-white/45">
            {actionCard("REVOKE_TAILSCALE_DEVICE_SHARE").notThis}
          </p>
        </details>
      ) : null}
    </section>
  );
}
