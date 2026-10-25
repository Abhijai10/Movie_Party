import { useState, type JSX, type SyntheticEvent } from "react";
import { motion } from "framer-motion";
import {
  AlertTriangle,
  ArrowLeft,
  Check,
  CircleDashed,
  ClipboardCopy,
  ExternalLink,
  Loader2,
  RefreshCw,
  ShieldCheck,
  Users,
  X,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import {
  TailscaleAccessPanel,
  type TailscaleAccessPanelProps,
} from "../components/mp/TailscaleAccessPanel";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import type {
  OnboardingMethod,
  OnboardingSide,
  OnboardingView,
  StoredFriend,
  WizardCheck,
  WizardIssue,
  WizardStage,
} from "../backend/appRuntime";
import {
  SHARE_LINK_APPLY_NOTE,
  STAGE_STEP_COPY,
  STAGE_TITLES,
  WIZARD_STAGES,
  checkRowLabel,
  checkRowTone,
  connectednessCopy,
  directionCopy,
  flowRows,
  handshakeEvidenceLine,
  headlineStatus,
  issueRow,
  keyExpiryCopy,
  methodCard,
  roleCopy,
  shareLinkSafetyCopy,
  sideWord,
  stageActions,
  stagePosition,
  topologyCopy,
} from "../onboarding/wizard";

/**
 * Batch 9 — the guided friend + Tailscale wizard.
 *
 * An "Operate" surface: every line here exists so the two people can tell what
 * is proved, what is waiting, and what only Tailscale can do. The decisions and
 * their wording live in `src/onboarding/wizard.ts` and the Rust reducer; this
 * file renders them and invents nothing. Specifically:
 *
 * - "Connected" is only ever the model's word, and the model reserves it for a
 *   proved Movie Party handshake (`view.connected`). A Tailscale status entry, a
 *   ping, or `progress.finished` alone never produces it.
 * - Requirement 17: nothing here installs Tailscale, creates a share, or accepts
 *   an invitation. The affordances are: open Tailscale's own page, open a pasted
 *   link, check again, check reachability, copy the invitation package.
 * - The footer renders exactly `stageActions(view)` — no phantom buttons.
 */

export type OnboardingWizardViewProps = {
  view: OnboardingView | null;
  friends: StoredFriend[];
  /** Action id currently in flight, as named by the shell. */
  busy: string | null;
  error: string | null;
  onBack: () => void;
  onStart: (peerKey: string, side: OnboardingSide) => void;
  onSetSide: (side: OnboardingSide) => void;
  onChooseMethod: (method: OnboardingMethod) => void;
  onOpenAdmin: () => void;
  onPasteShareLink: (link: string) => void;
  onOpenShareLink: () => void;
  onRefresh: () => void;
  onCheckReachability: () => void;
  onReportLinkExpired: () => void;
  onCopyPackage: () => void;
  onFinish: () => void;
  /**
   * Batch 10's access surface. Optional so the browser fixture and any callers
   * that only exercise the six stages keep working unchanged.
   */
  access?: TailscaleAccessPanelProps;
};

/** True when the shell reports any of these spellings as the in-flight action. */
type IsBusy = (tokens: readonly string[]) => boolean;

type Tone = "ok" | "wait" | "bad" | "off";

const TONE_TEXT: Record<Tone, string> = {
  ok: "text-emerald-300/85",
  wait: "text-amber-200/90",
  bad: "text-[#FCA5A5]",
  off: "text-white/40",
};

const CARD = "rounded-2xl border border-white/10 bg-white/[0.03] p-5";
const EYEBROW = "flex items-center gap-2 text-[11px] uppercase tracking-[0.28em] text-white/50";

function CheckGlyph({ tone }: { tone: Tone }): JSX.Element {
  if (tone === "ok") {
    return <Check className="w-4 h-4 text-emerald-300" strokeWidth={2.2} />;
  }
  if (tone === "bad") {
    return <X className="w-4 h-4 text-[#FCA5A5]" strokeWidth={2.2} />;
  }
  if (tone === "wait") {
    return <CircleDashed className="w-4 h-4 text-amber-200/90" strokeWidth={1.8} />;
  }
  return <CircleDashed className="w-4 h-4 text-white/35" strokeWidth={1.6} />;
}

function Spinner(): JSX.Element {
  return <Loader2 className="w-4 h-4 animate-spin shrink-0" aria-hidden="true" />;
}

function SectionEyebrow({
  icon: Icon,
  children,
}: {
  icon: LucideIcon;
  children: string;
}): JSX.Element {
  return (
    <span className={EYEBROW}>
      <Icon className="w-3.5 h-3.5 shrink-0" strokeWidth={1.6} />
      {children}
    </span>
  );
}

/** The reducer's ten rows, in the reducer's order, labelled by the model. */
function ChecklistColumn({ checks }: { checks: WizardCheck[] }): JSX.Element {
  return (
    <div className={CARD} data-testid="onboarding-checklist">
      <SectionEyebrow icon={ShieldCheck}>What Movie Party can see</SectionEyebrow>
      <ul className="mt-4 onboarding-checklist">
        {checks.map((check) => {
          const tone = checkRowTone(check.state);
          return (
            <li
              key={check.id}
              className="flex items-start gap-2.5"
              data-testid={`onboarding-check-${check.id}`}
              data-state={check.state}
              data-tone={tone}
            >
              <span className="mt-0.5 shrink-0">
                <CheckGlyph tone={tone} />
              </span>
              <span className="min-w-0 flex-1">
                <span className={`block text-xs leading-relaxed ${TONE_TEXT[tone]}`}>
                  {checkRowLabel(check.id)}
                </span>
                {check.state !== "PASSED" ? (
                  <span className="block text-[10px] tracking-[0.18em] uppercase text-white/40">
                    {check.state === "NOT_APPLICABLE"
                      ? "not applicable"
                      : check.state.toLowerCase()}
                    {check.code != null ? (
                      <span className="font-mono-mp"> · {check.code}</span>
                    ) : null}
                  </span>
                ) : null}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function IssueNotice({ issue }: { issue: WizardIssue }): JSX.Element {
  const row = issueRow(issue);
  return (
    <div
      className="rounded-xl border border-white/10 bg-white/[0.02] px-4 py-3.5"
      role="alert"
      data-testid={`onboarding-issue-${row.code}`}
      data-tone={row.tone}
      data-stage={issue.stage}
    >
      <div className="flex items-start gap-2.5">
        <span className="mt-0.5 shrink-0">
          <AlertTriangle className={`w-4 h-4 ${TONE_TEXT[row.tone]}`} strokeWidth={1.8} />
        </span>
        <div className="min-w-0">
          <p className="text-sm text-white/85">{row.headline}</p>
          <p className="mt-1 text-xs leading-relaxed text-white/55">{row.guidance}</p>
          <p className="mt-1 text-[10px] tracking-[0.18em] uppercase text-white/35">
            {row.code} · {issue.blocking ? "blocking" : "for information"}
          </p>
        </div>
      </div>
    </div>
  );
}

/**
 * Stage 1. The invitation itself is created and sent on the Friends screen; this
 * surface deliberately has no send affordance, because whether the friendship
 * exists is the other person's decision.
 */
function InviteStage({ view }: { view: OnboardingView }): JSX.Element {
  const inviteCheck = view.progress.checks.find((check) => check.id === "FRIEND_INVITED");
  return (
    <div className="space-y-4">
      <div className={CARD} data-testid="onboarding-invite-explainer">
        <SectionEyebrow icon={Users}>Friendship is their decision</SectionEyebrow>
        <p className="mt-3 text-sm leading-relaxed text-white/60">
          A friendship only exists once {view.displayName} accepts your signed invitation on their
          own device. The invitation is sent from the Friends screen — this wizard never sends,
          accepts, or revokes anything for either of you.
        </p>
        <p className="mt-2 text-xs leading-relaxed text-white/45">
          Once they have answered, use <span className="text-white/70">Check again</span> below:
          Movie Party re-reads everything it can see and moves on only when the friendship is
          actually recorded on this device.
        </p>
      </div>
      {inviteCheck != null ? (
        <div
          className="rounded-xl border border-white/10 bg-white/[0.02] px-4 py-3.5 flex items-start gap-2.5"
          data-testid="onboarding-invite-check"
          data-state={inviteCheck.state}
        >
          <span className="mt-0.5 shrink-0">
            <CheckGlyph tone={checkRowTone(inviteCheck.state)} />
          </span>
          <div className="min-w-0">
            <p className={`text-sm ${TONE_TEXT[checkRowTone(inviteCheck.state)]}`}>
              {checkRowLabel(inviteCheck.id)}
            </p>
            <p className="mt-1 text-[10px] tracking-[0.18em] uppercase text-white/40">
              {inviteCheck.state.toLowerCase()}
              {inviteCheck.code != null ? ` · ${inviteCheck.code}` : ""}
            </p>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function SideSwitch({
  mySide,
  isBusy,
  onSetSide,
}: {
  mySide: OnboardingSide;
  isBusy: IsBusy;
  onSetSide: (side: OnboardingSide) => void;
}): JSX.Element {
  const sideBusy = isBusy(["side"]);
  return (
    <div className={CARD} data-testid="onboarding-side-switch">
      <SectionEyebrow icon={ShieldCheck}>Which side is this device?</SectionEyebrow>
      <p className="mt-3 text-xs leading-relaxed text-white/55">
        Movie Party's {sideWord("GUEST")} is the only side that starts a connection, so which device
        was shared decides whether the direction can work at all.
      </p>
      <div className="mt-4 flex flex-wrap gap-3">
        <CinemaButton
          variant={mySide === "HOST" ? "primary" : "neutral"}
          disabled={sideBusy}
          onClick={() => {
            onSetSide("HOST");
          }}
          data-testid="onboarding-side-host"
          data-selected={String(mySide === "HOST")}
        >
          {sideBusy && mySide === "HOST" ? <Spinner /> : null}I'm the {sideWord("HOST")}
        </CinemaButton>
        <CinemaButton
          variant={mySide === "GUEST" ? "primary" : "neutral"}
          disabled={sideBusy}
          onClick={() => {
            onSetSide("GUEST");
          }}
          data-testid="onboarding-side-guest"
          data-selected={String(mySide === "GUEST")}
        >
          {sideBusy && mySide === "GUEST" ? <Spinner /> : null}I'm the {sideWord("GUEST")}
        </CinemaButton>
      </div>
    </div>
  );
}

/**
 * Stage 2. Grants AND limits are both rendered — a card that only listed what a
 * method gives would hide the reason the two are not interchangeable.
 */
function MethodStage({
  view,
  isBusy,
  onChooseMethod,
  onSetSide,
}: {
  view: OnboardingView;
  isBusy: IsBusy;
  onChooseMethod: (method: OnboardingMethod) => void;
  onSetSide: (side: OnboardingSide) => void;
}): JSX.Element {
  const methodBusy = isBusy(["method", "choose"]);
  return (
    <div className="space-y-4">
      <div className="grid gap-4 onboarding-methods">
        {view.methods.map((facts) => {
          const card = methodCard(facts, view.progress.method === facts.method, view.mySide);
          return (
            <article
              key={card.id}
              className={CARD}
              data-testid={`onboarding-method-${card.id}`}
              data-selected={String(card.selected)}
            >
              <h3 className="font-serif-display text-2xl text-white">{card.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-white/60">{card.summary}</p>
              <div className="mt-4">
                <SectionEyebrow icon={Check}>What it grants</SectionEyebrow>
                <ul className="mt-2 text-xs leading-relaxed text-white/60">
                  {card.grants.map((grant) => (
                    <li key={grant} className="onboarding-bullet">
                      {grant}
                    </li>
                  ))}
                </ul>
              </div>
              <div className="mt-4">
                <SectionEyebrow icon={AlertTriangle}>What it does not</SectionEyebrow>
                <ul className="mt-2 text-xs leading-relaxed text-amber-200/90">
                  {card.limits.map((limit) => (
                    <li key={limit} className="onboarding-bullet">
                      {limit}
                    </li>
                  ))}
                </ul>
              </div>
              <p className="mt-4 text-xs leading-relaxed text-white/55">{card.adminNote}</p>
              <p className="mt-1.5 text-xs leading-relaxed text-white/55">{card.consent}</p>
              <div className="mt-4">
                <CinemaButton
                  variant={card.selected ? "neutral" : "primary"}
                  disabled={methodBusy}
                  onClick={() => {
                    onChooseMethod(card.id);
                  }}
                  data-testid={`onboarding-method-select-${card.id}`}
                >
                  {methodBusy && card.selected ? <Spinner /> : null}
                  {card.selected ? "Selected" : "Choose this way"}
                </CinemaButton>
              </div>
            </article>
          );
        })}
      </div>
      <SideSwitch mySide={view.mySide} isBusy={isBusy} onSetSide={onSetSide} />
    </div>
  );
}

/**
 * Stage 3. A pasted link is a bearer credential: only the model's metadata about
 * it is rendered, the field is cleared as soon as the link has been handed over,
 * and the raw text never reaches the DOM or stays in the input.
 */
function ApprovalStage({
  view,
  isBusy,
  onPasteShareLink,
  onOpenShareLink,
  onReportLinkExpired,
}: {
  view: OnboardingView;
  isBusy: IsBusy;
  onPasteShareLink: (link: string) => void;
  onOpenShareLink: () => void;
  onReportLinkExpired: () => void;
}): JSX.Element {
  const [linkText, setLinkText] = useState("");
  const pasteBusy = isBusy(["paste", "share-link"]);
  const openBusy = isBusy(["openlink", "open-link", "openshare"]);
  const reportBusy = isBusy(["expired"]);
  const canOpenLink = view.shareLink?.kind === "TAILSCALE_INVITE";

  const submit = (event: SyntheticEvent<HTMLFormElement>): void => {
    event.preventDefault();
    const raw = linkText.trim();
    if (raw.length === 0 || pasteBusy) return;
    onPasteShareLink(raw);
    setLinkText("");
  };

  return (
    <div className="space-y-4">
      <div className={CARD}>
        <SectionEyebrow icon={ExternalLink}>Tailscale does this part</SectionEyebrow>
        <p className="mt-3 text-sm leading-relaxed text-white/60">
          The share or invitation is created and accepted inside Tailscale, with the other person's
          own account. Movie Party opens Tailscale's page for you (the footer button) and re-reads
          what happened afterwards — it cannot create the access, and it cannot claim it for you.
        </p>
      </div>

      <form className={CARD} onSubmit={submit} data-testid="onboarding-share-link-form">
        <SectionEyebrow icon={Users}>Already have a link?</SectionEyebrow>
        <label htmlFor="onboarding-share-link-input" className="mt-3 block text-xs text-white/60">
          Paste the Tailscale share or invite link. Movie Party only identifies what it is.
        </label>
        <div className="mt-3 flex flex-wrap gap-3">
          <input
            id="onboarding-share-link-input"
            value={linkText}
            onChange={(event) => {
              setLinkText(event.target.value);
            }}
            type="url"
            autoComplete="off"
            spellCheck={false}
            placeholder="https://your-tailnet.login.tailscale.com/a/…"
            data-testid="onboarding-share-link-input"
            className="mp-input font-mono-mp"
          />
          <CinemaButton type="submit" disabled={linkText.trim().length === 0 || pasteBusy}>
            {pasteBusy ? <Spinner /> : null}
            {pasteBusy ? "Checking…" : "Check the link"}
          </CinemaButton>
        </div>
      </form>

      <div className={CARD} data-testid="onboarding-share-link-safety">
        <SectionEyebrow icon={ShieldCheck}>What happened to that link</SectionEyebrow>
        <p className="mt-3 text-sm leading-relaxed text-white/60">
          {shareLinkSafetyCopy(view.shareLink)}
        </p>
        <p className="mt-2 text-xs leading-relaxed text-white/45">{SHARE_LINK_APPLY_NOTE}</p>
        {canOpenLink ? (
          <div className="mt-4 flex flex-wrap gap-3">
            <CinemaButton
              variant="ghost"
              disabled={openBusy}
              onClick={() => {
                onOpenShareLink();
              }}
              data-testid="onboarding-open-share-link"
            >
              {openBusy ? <Spinner /> : null}
              Open the link in my browser
            </CinemaButton>
          </div>
        ) : null}
        <div className="mt-4">
          <CinemaButton
            variant="neutral"
            disabled={reportBusy}
            onClick={() => {
              onReportLinkExpired();
            }}
            data-testid="onboarding-report-expired"
          >
            {reportBusy ? <Spinner /> : null}
            They told me the link didn't work
          </CinemaButton>
        </div>
      </div>
    </div>
  );
}

/**
 * Stage 4. Topology, direction and the per-flow table come straight from the
 * model, including its "Direction unknown" and "not verified — needs two
 * devices". The reachability and check-again buttons live in the footer, so each
 * appears exactly once per stage.
 */
function ReachabilityStage({ view }: { view: OnboardingView }): JSX.Element {
  const direction = directionCopy(view.progress.direction, view.directionEvidence);
  const rows = flowRows(view.flows);
  const probe = view.probe;
  return (
    <div className="space-y-4">
      <div className={CARD}>
        <SectionEyebrow icon={Users}>How the two devices sit</SectionEyebrow>
        <p className="mt-3 text-sm leading-relaxed text-white/60" data-testid="onboarding-topology">
          {topologyCopy(view.progress.topology)}
        </p>
      </div>

      <div className={CARD} data-testid="onboarding-direction">
        <SectionEyebrow icon={AlertTriangle}>Connection direction</SectionEyebrow>
        <p
          className="mt-3 font-serif-display text-2xl text-white"
          data-testid="onboarding-direction-headline"
        >
          {direction.headline}
        </p>
        <p className="mt-2 text-xs leading-relaxed text-white/55">{direction.detail}</p>
        {direction.remedy != null ? (
          <p
            className="mt-2 text-xs leading-relaxed text-amber-200/90"
            data-testid="onboarding-direction-remedy"
          >
            {direction.remedy}
          </p>
        ) : null}
      </div>

      <div className={CARD} data-testid="onboarding-flows">
        <SectionEyebrow icon={ShieldCheck}>Who starts each connection</SectionEyebrow>
        <div className="mt-3 onboarding-scroll-x">
          <table className="onboarding-flow-table" data-testid="onboarding-flow-table">
            <thead>
              <tr>
                <th scope="col">Connection</th>
                <th scope="col">Who starts it</th>
                <th scope="col">Reverse needed</th>
                <th scope="col">Evidence</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.name} data-testid={`onboarding-flow-${row.name}`}>
                  <th scope="row">{row.name}</th>
                  <td>{row.starts}</td>
                  <td>{row.reverse}</td>
                  <td>{row.evidence}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {probe != null ? (
        <div
          className={CARD}
          data-testid="onboarding-probe"
          data-reachable={String(probe.reachable)}
        >
          <SectionEyebrow icon={RefreshCw}>Last reachability check</SectionEyebrow>
          <p className={`mt-3 text-sm ${probe.reachable ? "text-white/80" : "text-amber-200/90"}`}>
            {probe.message}
          </p>
          <p className="mt-1 text-[11px] tracking-[0.18em] uppercase text-white/40">
            {probe.path != null ? `path ${probe.path}` : "no path reported"}
            {probe.latencyMs != null ? ` · ${String(probe.latencyMs)} ms` : ""}
          </p>
          <p className="mt-2 text-xs leading-relaxed text-white/45">
            An answer through the tunnel proves the network reached a device. It does not prove that
            device is the one Movie Party trusts — that is the next step.
          </p>
        </div>
      ) : (
        <div className={CARD}>
          <p className="text-xs leading-relaxed text-white/45" data-testid="onboarding-probe-none">
            No reachability check has run in this wizard session yet. Use{" "}
            <span className="text-white/70">Check reachability</span> below, then{" "}
            <span className="text-white/70">Check again</span> to re-read everything.
          </p>
        </div>
      )}
    </div>
  );
}

/**
 * Stage 5. The model's wording: a real transport handshake finishes this step,
 * and a tunnel that merely reaches the device is shown as "Tunnel only".
 */
function IdentityStage({ view }: { view: OnboardingView }): JSX.Element {
  const status = connectednessCopy(view.progress.connectedness, view);
  return (
    <div className="space-y-4">
      <div className={CARD} data-testid="onboarding-connectedness" data-tone={status.tone}>
        <SectionEyebrow icon={ShieldCheck}>Movie Party's own check</SectionEyebrow>
        <p
          className="mt-3 font-serif-display text-3xl text-white"
          data-testid="onboarding-connectedness-label"
        >
          {status.label}
        </p>
        <p className="mt-2 text-xs leading-relaxed text-white/55">{status.detail}</p>
      </div>
      <div className={CARD}>
        <p
          className="text-sm leading-relaxed text-white/60"
          data-testid="onboarding-handshake-evidence"
        >
          {handshakeEvidenceLine(view)}
        </p>
        <p className="mt-2 text-xs leading-relaxed text-white/45">
          This step finishes when one device holds a party and the other joins it, so the room
          transport performs an authenticated handshake across the tunnel. A Tailscale status entry
          or a ping answers a different question and never completes it.
        </p>
      </div>
    </div>
  );
}

/**
 * Stage 6. The connected banner is only reachable from here, because the reducer
 * only puts the wizard at this stage once the handshake is proved.
 */
function ReadyStage({ view, isBusy }: { view: OnboardingView; isBusy: IsBusy }): JSX.Element {
  const status = connectednessCopy(view.progress.connectedness, view);
  const packageBusy = isBusy(["package", "copy"]);
  return (
    <div className="space-y-4">
      {view.connected ? (
        <div
          className="rounded-2xl border border-[#9F7AEA]/35 onboarding-connected p-5"
          data-testid="onboarding-connected"
        >
          <div className="flex items-center gap-3">
            <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full border border-[#9F7AEA]/35 bg-[#6B46C1]/20">
              <ShieldCheck className="h-5 w-5 text-[#C4B5FD]" strokeWidth={1.8} />
            </span>
            <div className="min-w-0">
              <p className="font-serif-display text-2xl text-white">{status.label}</p>
              <p className="mt-1 text-xs leading-relaxed text-white/60">{status.detail}</p>
            </div>
          </div>
        </div>
      ) : (
        <div className={CARD}>
          <p className="text-sm leading-relaxed text-white/60">{status.label}</p>
          <p className="mt-1 text-xs leading-relaxed text-white/45">{status.detail}</p>
        </div>
      )}
      <div className={CARD} data-testid="onboarding-package-note">
        <SectionEyebrow icon={ClipboardCopy}>The invitation package</SectionEyebrow>
        <p className="mt-3 text-xs leading-relaxed text-white/55">
          <span className="text-white/75">Copy invitation package</span> below puts everything{" "}
          {view.displayName} needs into one message you can send yourself. It carries your signed
          friendship invitation and this wizard's facts — never the Tailscale share link — and
          copying it hands nothing to anyone automatically.
        </p>
        {packageBusy ? (
          <p className="mt-2 flex items-center gap-2 text-[11px] uppercase tracking-[0.18em] text-white/45">
            <Spinner />
            Building the package
          </p>
        ) : null}
      </div>
    </div>
  );
}

function StageBody({
  view,
  isBusy,
  handlers,
}: {
  view: OnboardingView;
  isBusy: IsBusy;
  handlers: Pick<
    OnboardingWizardViewProps,
    "onChooseMethod" | "onSetSide" | "onPasteShareLink" | "onOpenShareLink" | "onReportLinkExpired"
  >;
}): JSX.Element {
  const stage: WizardStage = view.progress.stage;
  switch (stage) {
    case "INVITE_FRIEND":
      return <InviteStage view={view} />;
    case "CHOOSE_METHOD":
      return (
        <MethodStage
          view={view}
          isBusy={isBusy}
          onChooseMethod={handlers.onChooseMethod}
          onSetSide={handlers.onSetSide}
        />
      );
    case "TAILSCALE_APPROVAL":
      return (
        <ApprovalStage
          view={view}
          isBusy={isBusy}
          onPasteShareLink={handlers.onPasteShareLink}
          onOpenShareLink={handlers.onOpenShareLink}
          onReportLinkExpired={handlers.onReportLinkExpired}
        />
      );
    case "PEER_REACHABILITY":
      return <ReachabilityStage view={view} />;
    case "IDENTITY_VERIFICATION":
      return <IdentityStage view={view} />;
    case "FRIEND_READY":
      return <ReadyStage view={view} isBusy={isBusy} />;
  }
}

type FooterAction = {
  key: string;
  testid: string;
  label: string;
  busyLabel: string;
  busyTokens: readonly string[];
  variant: "primary" | "ghost" | "neutral";
  icon: LucideIcon | undefined;
  onClick: () => void;
};

type FooterHandlers = Pick<
  OnboardingWizardViewProps,
  "onBack" | "onOpenAdmin" | "onRefresh" | "onCheckReachability" | "onCopyPackage" | "onFinish"
>;

/**
 * One footer control per entry `stageActions` returns, matched by name rather
 * than by a checked-over-the-union `switch`: the action set is the model's to
 * change, and a view must not fail to build because a name came or went. An
 * unrecognised name renders nothing rather than a guessed button. Sending an
 * invitation is deliberately not one of these actions — it belongs to the
 * Friends screen, and this surface must not claim to do it (requirement 17).
 */
function footerAction(action: string, handlers: FooterHandlers): FooterAction | null {
  if (action === "OPEN_TAILSCALE_ADMIN") {
    return {
      key: action,
      testid: "onboarding-open-admin",
      label: "Open Tailscale's page",
      busyLabel: "Opening Tailscale…",
      busyTokens: ["admin", "open-tailscale"],
      variant: "primary",
      icon: ExternalLink,
      onClick: () => {
        handlers.onOpenAdmin();
      },
    };
  }
  if (action === "CHECK_AGAIN") {
    return {
      key: action,
      testid: "onboarding-refresh",
      label: "Check again",
      busyLabel: "Checking…",
      busyTokens: ["again", "refresh"],
      variant: "neutral",
      icon: RefreshCw,
      onClick: () => {
        handlers.onRefresh();
      },
    };
  }
  if (action === "CHECK_REACHABILITY") {
    return {
      key: action,
      testid: "onboarding-check-reachability",
      label: "Check reachability",
      busyLabel: "Checking the tunnel…",
      busyTokens: ["reachability", "reachable"],
      variant: "primary",
      icon: ShieldCheck,
      onClick: () => {
        handlers.onCheckReachability();
      },
    };
  }
  if (action === "COPY_PACKAGE") {
    return {
      key: action,
      testid: "onboarding-copy-package",
      label: "Copy invitation package",
      busyLabel: "Copying…",
      busyTokens: ["package", "copy"],
      variant: "primary",
      icon: ClipboardCopy,
      onClick: () => {
        handlers.onCopyPackage();
      },
    };
  }
  if (action === "FINISH") {
    return {
      key: action,
      testid: "onboarding-finish",
      label: "Finish setup",
      busyLabel: "Finishing…",
      busyTokens: ["finish"],
      variant: "primary",
      icon: Check,
      onClick: () => {
        handlers.onFinish();
      },
    };
  }
  return null;
}

function ActionFooter({
  view,
  isBusy,
  handlers,
}: {
  view: OnboardingView;
  isBusy: IsBusy;
  handlers: FooterHandlers;
}): JSX.Element {
  const actionNames: string[] = stageActions(view);
  const descriptors: FooterAction[] = actionNames
    .map((action) => footerAction(action, handlers))
    .filter((entry): entry is FooterAction => entry != null);

  return (
    <div className="mt-5 flex flex-wrap items-center gap-3" data-testid="onboarding-actions">
      {descriptors.map((action) => {
        const inFlight = action.busyTokens.length > 0 && isBusy(action.busyTokens);
        return (
          <CinemaButton
            key={action.key}
            variant={action.variant}
            icon={inFlight ? undefined : action.icon}
            iconPos="left"
            disabled={inFlight}
            onClick={action.onClick}
            data-testid={action.testid}
            data-busy={String(inFlight)}
            data-action={action.key}
          >
            {inFlight ? <Spinner /> : null}
            {inFlight ? action.busyLabel : action.label}
          </CinemaButton>
        );
      })}
    </div>
  );
}

/** Pre-wizard panel: pick the friend and this device's side, then begin. */
function StartPanel({
  friends,
  busy,
  isBusy,
  onStart,
}: {
  friends: StoredFriend[];
  busy: string | null;
  isBusy: IsBusy;
  onStart: (peerKey: string, side: OnboardingSide) => void;
}): JSX.Element {
  const [peerKey, setPeerKey] = useState("");
  const [side, setSide] = useState<OnboardingSide | null>(null);
  const startBusy = isBusy(["start", "begin"]);
  const canStart = peerKey.length > 0 && side != null;

  return (
    <div className="grid gap-4 onboarding-methods" data-testid="onboarding-start-panel">
      <div className={CARD}>
        <SectionEyebrow icon={Users}>Who are you setting up?</SectionEyebrow>
        {friends.length === 0 ? (
          <p
            className="mt-3 text-sm leading-relaxed text-white/50"
            data-testid="onboarding-no-friends"
          >
            No friends are saved on this device yet. Invite one from the Friends screen first — the
            wizard walks an existing friendship onto a private network, it cannot create one.
          </p>
        ) : (
          <ul className="mt-4 space-y-3" data-testid="onboarding-peer-picker">
            {friends.map((friend) => {
              const selected = friend.peerKey === peerKey;
              return (
                <li key={friend.peerKey} className="min-w-0">
                  <CinemaButton
                    variant={selected ? "primary" : "neutral"}
                    icon={selected ? Check : undefined}
                    iconPos="left"
                    onClick={() => {
                      setPeerKey(friend.peerKey);
                    }}
                    data-testid={`onboarding-peer-${friend.peerKey}`}
                    data-selected={String(selected)}
                  >
                    {friend.displayName}
                  </CinemaButton>
                  <p className="mt-1.5 text-[11px] text-white/40 font-mono-mp break-words">
                    {friend.peerKey}
                  </p>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <div className={CARD}>
        <SectionEyebrow icon={ShieldCheck}>Which side is this device?</SectionEyebrow>
        <p className="mt-3 text-xs leading-relaxed text-white/55">
          You can change this later — the wizard asks again before it judges the share direction.
        </p>
        <div className="mt-4 flex flex-wrap gap-3">
          <CinemaButton
            variant={side === "HOST" ? "primary" : "neutral"}
            disabled={startBusy}
            onClick={() => {
              setSide("HOST");
            }}
            data-testid="onboarding-side-host"
            data-selected={String(side === "HOST")}
          >
            I'm the {sideWord("HOST")}
          </CinemaButton>
          <CinemaButton
            variant={side === "GUEST" ? "primary" : "neutral"}
            disabled={startBusy}
            onClick={() => {
              setSide("GUEST");
            }}
            data-testid="onboarding-side-guest"
            data-selected={String(side === "GUEST")}
          >
            I'm the {sideWord("GUEST")}
          </CinemaButton>
        </div>
      </div>

      <div className={CARD}>
        <SectionEyebrow icon={ExternalLink}>What the wizard does</SectionEyebrow>
        <p className="mt-3 text-xs leading-relaxed text-white/55">
          It walks from "invite a friend" to "both devices are connected and verified", for either
          way you and your friend use Tailscale, and reports what each step actually proved.
          Everything that happens inside Tailscale stays inside Tailscale: Movie Party opens its
          page, then checks again.
        </p>
        <div className="mt-4">
          <CinemaButton
            variant="primary"
            disabled={!canStart || busy != null}
            onClick={() => {
              if (side == null || peerKey.length === 0) return;
              onStart(peerKey, side);
            }}
            data-testid="onboarding-start"
            data-ready={String(canStart)}
          >
            {startBusy ? <Spinner /> : null}
            Start the wizard
          </CinemaButton>
        </div>
        {!canStart ? (
          <p
            className="mt-2 text-[11px] tracking-[0.18em] uppercase text-white/35"
            data-testid="onboarding-start-hint"
          >
            pick a friend and a side to start
          </p>
        ) : null}
      </div>
    </div>
  );
}

/** The six stages, in model order, with the current one marked. */
function StageRail({ stage }: { stage: WizardStage }): JSX.Element {
  const current = stagePosition(stage).index;
  return (
    <ol className="onboarding-rail" data-testid="onboarding-rail" data-current={stage}>
      {WIZARD_STAGES.map((entry, index) => (
        <li
          key={entry}
          data-testid={`onboarding-rail-${entry}`}
          data-state={index < current ? "past" : index === current ? "current" : "ahead"}
        >
          <span className="onboarding-rail-dot" aria-hidden="true" />
          <span className="onboarding-rail-label">{STAGE_TITLES[entry]}</span>
        </li>
      ))}
    </ol>
  );
}

export function OnboardingWizardView({
  view,
  friends,
  busy,
  error,
  onBack,
  onStart,
  onSetSide,
  onChooseMethod,
  onOpenAdmin,
  onPasteShareLink,
  onOpenShareLink,
  onRefresh,
  onCheckReachability,
  onReportLinkExpired,
  onCopyPackage,
  onFinish,
  access,
}: OnboardingWizardViewProps): JSX.Element {
  const isBusy: IsBusy = (tokens) => {
    if (busy === null) return false;
    const needle = busy.toLowerCase();
    return tokens.some((token) => needle.includes(token));
  };
  const isActive = view != null && view.active;
  const status = view != null ? headlineStatus(view) : null;
  const position = view != null ? stagePosition(view.progress.stage) : null;

  return (
    <div
      className="relative w-full min-h-screen flex flex-col bg-[#05050B] text-white"
      data-testid="onboarding-wizard"
      data-active={String(isActive)}
    >
      <div className="fixed inset-0 pointer-events-none">
        <SilkBackground variant="calm" />
      </div>

      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <CinemaButton
          variant="ghost"
          className="onboarding-back"
          icon={ArrowLeft}
          iconPos="left"
          onClick={() => {
            onBack();
          }}
          data-testid="onboarding-back"
        >
          Back
        </CinemaButton>
        {view != null && status != null ? (
          <StatusIndicator
            state={status.tone === "ok" ? "ready" : status.tone === "wait" ? "waiting" : "idle"}
            label={STAGE_TITLES[view.progress.stage]}
          />
        ) : (
          <StatusIndicator state="idle" label="Friend setup" />
        )}
      </header>

      <main className="relative z-10 max-w-[1100px] w-full mx-auto px-12 mt-6 pb-20 flex-1">
        <motion.section
          initial={{ opacity: 0, y: 14 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
        >
          <span className={EYEBROW}>
            <Users className="w-3.5 h-3.5 shrink-0" strokeWidth={1.6} />
            {isActive ? "Friend + private network setup" : "Start a guided setup"}
          </span>
          <h1 className="mt-4 font-serif-display text-4xl tracking-[-0.02em] text-white">
            {view != null ? STAGE_TITLES[view.progress.stage] : "Connect you and your friend"}
          </h1>
          <p
            className="mt-3 text-sm leading-relaxed text-white/60 max-w-2xl"
            data-testid="onboarding-stage"
          >
            {view != null
              ? STAGE_STEP_COPY[view.progress.stage]
              : "One guided path from an invitation to both devices being connected and verified — for the case where you share one machine, and for the case where the other device joins the same tailnet."}
          </p>

          {error != null ? (
            <p
              role="alert"
              className="mt-4 text-sm leading-relaxed text-[#FCA5A5]"
              data-testid="onboarding-error"
            >
              {error}
            </p>
          ) : null}

          {view != null && view.active && status != null && position != null ? (
            <>
              <div className="mt-6 flex flex-wrap items-center gap-3">
                <p
                  className="text-[11px] tracking-[0.22em] uppercase text-white/50"
                  data-testid="onboarding-step-count"
                  data-step={String(position.index + 1)}
                  data-total={String(position.total)}
                >
                  Step {String(position.index + 1)} of {String(position.total)}
                </p>
                <p
                  className={`text-sm leading-relaxed ${TONE_TEXT[status.tone]}`}
                  data-testid="onboarding-headline"
                  data-tone={status.tone}
                >
                  {status.text}
                </p>
              </div>
              <StageRail stage={view.progress.stage} />

              <div className="mt-6 rounded-2xl border border-white/10 bg-white/[0.03] p-5">
                <div className="flex flex-wrap items-start justify-between gap-4">
                  <div className="min-w-0">
                    <SectionEyebrow icon={Users}>Setting up with</SectionEyebrow>
                    <p
                      className="mt-2 text-lg text-white/90 break-words"
                      data-testid="onboarding-peer-name"
                    >
                      {view.displayName}
                    </p>
                    <p className="mt-1 text-[11px] text-white/40 font-mono-mp break-words">
                      {view.peerKey}
                    </p>
                  </div>
                  <dl className="grid gap-1.5 text-xs min-w-0">
                    <div className="flex flex-wrap gap-2">
                      <dt className="tracking-[0.2em] uppercase text-white/40">Your identity</dt>
                      <dd
                        className="font-mono-mp text-white/75"
                        data-testid="onboarding-fingerprint-us"
                      >
                        {view.ourFingerprint}
                      </dd>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <dt className="tracking-[0.2em] uppercase text-white/40">Theirs</dt>
                      <dd
                        className="font-mono-mp text-white/75 break-words"
                        data-testid="onboarding-fingerprint-them"
                        data-known={String(view.theirFingerprint != null)}
                      >
                        {view.theirFingerprint ?? "not yet known"}
                      </dd>
                    </div>
                  </dl>
                </div>
                <div className="mt-4 grid gap-1.5 border-t border-white/10 pt-4">
                  <p
                    className="text-xs leading-relaxed text-white/55"
                    data-testid="onboarding-tailnet-role"
                  >
                    {roleCopy(view.tailnet?.role)}
                  </p>
                  <p
                    className="text-xs leading-relaxed text-white/55"
                    data-testid="onboarding-node-key"
                  >
                    {keyExpiryCopy(view.nodeKey)}
                  </p>
                  {view.tailnet == null ? (
                    <p
                      className="text-xs leading-relaxed text-white/40"
                      data-testid="onboarding-tailnet-unknown"
                    >
                      Movie Party has no tailnet reading for this device — Tailscale reported
                      nothing it could use, so nothing here assumes otherwise.
                    </p>
                  ) : (
                    <p
                      className="text-xs text-white/40 font-mono-mp break-words"
                      data-testid="onboarding-tailnet-handle"
                    >
                      {view.tailnet.magicDnsSuffix ?? "no MagicDNS suffix reported"}
                      {view.tailnet.keyExpiry != null
                        ? ` · key expiry ${view.tailnet.keyExpiry}`
                        : ""}
                    </p>
                  )}
                </div>
              </div>

              <div className="mt-6 onboarding-layout">
                <ChecklistColumn checks={view.progress.checks} />
                <div className="min-w-0">
                  {view.progress.issues.length > 0 ? (
                    <div className="space-y-3 mb-4" data-testid="onboarding-issues">
                      {view.progress.issues.map((issue) => (
                        <IssueNotice key={issue.id} issue={issue} />
                      ))}
                    </div>
                  ) : null}
                  <StageBody
                    view={view}
                    isBusy={isBusy}
                    handlers={{
                      onChooseMethod,
                      onSetSide,
                      onPasteShareLink,
                      onOpenShareLink,
                      onReportLinkExpired,
                    }}
                  />
                  {access != null && view.progress.stage === "TAILSCALE_APPROVAL" ? (
                    <div className="mt-4">
                      <TailscaleAccessPanel {...access} />
                    </div>
                  ) : null}
                  <ActionFooter
                    view={view}
                    isBusy={isBusy}
                    handlers={{
                      onBack,
                      onOpenAdmin,
                      onRefresh,
                      onCheckReachability,
                      onCopyPackage,
                      onFinish,
                    }}
                  />
                </div>
              </div>
            </>
          ) : (
            <div className="mt-6">
              <StartPanel friends={friends} busy={busy} isBusy={isBusy} onStart={onStart} />
            </div>
          )}
        </motion.section>
      </main>
    </div>
  );
}

export default OnboardingWizardView;
