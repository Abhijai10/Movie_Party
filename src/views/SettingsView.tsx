import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion } from "framer-motion";
import { ArrowLeft, Download, RotateCcw, Trash2 } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import {
  checkProviderStatus,
  listProviderDiagnostics,
  runProviderSharedDiagnostic,
  type StoredProviderDiagnostic,
  getAppMetadataInfo,
  getProviderCapabilities,
  getTailscaleReadiness,
  openProviderBrowser,
  setDisplayName,
  type AppSnapshot,
  type ProviderCapability,
  type TailscaleReadiness,
} from "../backend/appRuntime";
import {
  bundledTmdbToken,
  clearTmdbCache,
  getTmdbToken,
  readTmdbStatus,
  setTmdbToken,
} from "../home/tmdbFeed";

/**
 * UI_UX_SPEC §55–§62 — Settings.
 *
 * Product-register rules: consistent CinemaButton vocabulary, state-rich
 * but restrained color (amber for destructive-confirm, sky for info),
 * no decorative motion. Truth rules: providers show empirical support
 * (§38), Strict Sync is visible but NOT disableable (§56 — it is
 * core product behavior, §14), destructive actions confirm.
 */
type SettingsViewProps = {
  snapshot: AppSnapshot;
  onBack: () => void;
  /** Applied after a successful rename so the whole app sees the new name. */
  onSnapshot?: (next: AppSnapshot | null) => void;
};

type SettingsSection =
  | "general"
  | "playback"
  | "network"
  | "call"
  | "storage"
  | "providers"
  | "privacy"
  | "diagnostics";

const SECTIONS: Array<{ id: SettingsSection; label: string }> = [
  { id: "general", label: "General" },
  { id: "playback", label: "Playback" },
  { id: "network", label: "Network" },
  { id: "call", label: "Call" },
  { id: "storage", label: "Storage" },
  { id: "providers", label: "Providers" },
  { id: "privacy", label: "Privacy" },
  { id: "diagnostics", label: "Diagnostics" },
];

function Row({ label, children, hint }: { label: string; children?: React.ReactNode; hint?: string }) {
  return (
    <div className="flex items-center justify-between gap-8 py-3 border-b border-white/[0.06] last:border-b-0">
      <div className="min-w-0">
        <p className="text-sm text-white/85">{label}</p>
        {hint ? <p className="text-xs text-white/40 mt-0.5">{hint}</p> : null}
      </div>
      {children ? (
        <div className="shrink-0 text-sm text-white/60 text-right">{children}</div>
      ) : null}
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** exponent;
  const label = value >= 10 ? String(Math.round(value)) : value.toFixed(1);
  return `${label} ${units[exponent] ?? "B"}`;
}

export function SettingsView({ snapshot, onBack, onSnapshot }: SettingsViewProps) {
  const [section, setSection] = useState<SettingsSection>("general");

  // Display-name editing (General): the row is an inline text field that
  // saves on Enter / blur. Validation lives in the backend (1–40 chars); a
  // rejection surfaces inline rather than silently reverting.
  const [nameDraft, setNameDraft] = useState<string>(
    snapshot.participants[0]?.displayName ?? "You",
  );
  const [nameSaving, setNameSaving] = useState(false);
  const [nameError, setNameError] = useState<string | null>(null);
  useEffect(() => {
    setNameDraft(snapshot.participants[0]?.displayName ?? "You");
  }, [snapshot.participants]);

  const saveName = useCallback(() => {
    const trimmed = nameDraft.trim();
    if (trimmed.length === 0 || trimmed.length > 40) {
      setNameError("Name must be 1–40 characters.");
      return;
    }
    const currentName = snapshot.participants[0]?.displayName ?? "You";
    if (trimmed === currentName) {
      setNameError(null);
      return;
    }
    setNameSaving(true);
    setNameError(null);
    void setDisplayName(trimmed).then((next) => {
      setNameSaving(false);
      if (next) {
        onSnapshot?.(next);
      } else {
        setNameError("Could not save the name right now.");
      }
    });
  }, [nameDraft, snapshot.participants, onSnapshot]);

  const [capabilities, setCapabilities] = useState<ProviderCapability[]>([]);
  const [tailscale, setTailscale] = useState<TailscaleReadiness | null>(null);
  const [metadata, setMetadata] = useState<{ appVersion: string; protocol: string }>({
    appVersion: "—",
    protocol: "—",
  });
  const [confirmResetProvider, setConfirmResetProvider] = useState<string | null>(null);
  const [exportedBundle, setExportedBundle] = useState<string | null>(null);
  // TMDB hero key: stored only in localStorage on this device.
  const [tmdbTokenSaved, setTmdbTokenSaved] = useState<boolean>(() =>
    getTmdbToken(window.localStorage).length > 0,
  );
  // Diagnostics for the last live fetch: distinguishes "key saved but
  // TMDB unreachable" (common: some ISPs block themoviedb.org) from a
  // working feed — read fresh whenever the row re-renders.
  const tmdbStatus = readTmdbStatus(window.localStorage);
  const tmdbLiveOk =
    tmdbStatus != null &&
    tmdbStatus.lastSuccessAtMs != null &&
    tmdbStatus.lastAttemptAtMs === tmdbStatus.lastSuccessAtMs;
  const [tmdbDialogOpen, setTmdbDialogOpen] = useState(false);
  const [tmdbDraft, setTmdbDraft] = useState("");
  // Provider Shared diagnostic records + run state.
  const [diagnostics, setDiagnostics] = useState<Record<string, StoredProviderDiagnostic>>({});
  const [diagnosticRunning, setDiagnosticRunning] = useState<string | null>(null);
  const [diagnosticMessage, setDiagnosticMessage] = useState<string | null>(null);
  const exportTimer = useRef<number | undefined>(undefined);

  const refreshDiagnostics = useCallback(() => {
    void listProviderDiagnostics().then((records) => {
      const map: Record<string, StoredProviderDiagnostic> = {};
      for (const record of records) {
        map[record.providerId] = record;
      }
      setDiagnostics(map);
    });
  }, []);

  useEffect(() => {
    refreshDiagnostics();
  }, [refreshDiagnostics]);

  const runDiagnostic = (providerId: string) => {
    if (diagnosticRunning != null) {
      return;
    }
    setDiagnosticRunning(providerId);
    setDiagnosticMessage(null);
    void runProviderSharedDiagnostic(providerId).then((record) => {
      setDiagnosticRunning(null);
      if (record == null) {
        // The wrapper logs the raw error; surface the honest reason the
        // run could not classify (§29 — never a silent failure).
        setDiagnosticMessage(
          "The diagnostic could not run — see the reason in the app log (ffmpeg missing or capture permission denied are the common causes).",
        );
        return;
      }
      setDiagnostics((current) => ({ ...current, [record.providerId]: record }));
      setDiagnosticMessage(
        record.sharedAvailable
          ? `Shared Mode verified for ${record.displayName} on this device.`
          : record.sharedReason,
      );
    });
  };

  const saveTmdbToken = () => {
    setTmdbToken(window.localStorage, tmdbDraft);
    clearTmdbCache(window.localStorage);
    setTmdbTokenSaved(tmdbDraft.trim().length > 0);
    setTmdbDialogOpen(false);
    setTmdbDraft("");
  };

  const clearTmdbToken = () => {
    setTmdbToken(window.localStorage, "");
    clearTmdbCache(window.localStorage);
    setTmdbTokenSaved(false);
  };

  useEffect(() => {
    let cancelled = false;
    void getProviderCapabilities().then((caps) => {
      if (!cancelled) setCapabilities(caps);
    });
    void getTailscaleReadiness().then((readiness) => {
      if (!cancelled) setTailscale(readiness);
    });
    void getAppMetadataInfo().then((info) => {
      if (!cancelled && info) {
        setMetadata({
          appVersion: info.appName,
          protocol: `V${String(info.protocolMajor)}.${String(info.protocolMinor)}`,
        });
      }
    });
    return () => {
      cancelled = true;
      window.clearTimeout(exportTimer.current);
    };
  }, []);

  // Diagnostics bundle export (§62): assembled locally from live snapshot +
  // metadata — nothing leaves the machine.
  const diagnosticBundle = useMemo(
    () =>
      JSON.stringify(
        {
          exportedAtUtcMs: Date.now(),
          app: metadata,
          network: snapshot.network,
          room: {
            role: snapshot.room.role,
            state: snapshot.sync.roomState,
            strictSync: snapshot.room.strictSync,
          },
          media: snapshot.media
            ? { filename: snapshot.media.filename, fileSize: snapshot.media.fileSize }
            : null,
          provider: snapshot.provider,
          call: {
            mode: snapshot.call.mode,
            status: snapshot.call.status,
            cameraTier: snapshot.call.camera.tier,
          },
        },
        null,
        2,
      ),
    [metadata, snapshot],
  );

  // §62 Export Diagnostic Bundle — saved locally via the browser File
  // System API when available; otherwise the bundle is copied to the
  // clipboard (still local-only, never uploaded).
  const exportBundle = () => {
    void (async () => {
      try {
        const picker = (
          window as unknown as {
            showSaveFilePicker?: (options: {
              suggestedName: string;
            }) => Promise<{
              createWritable: () => Promise<{
                write: (data: Blob) => Promise<void>;
                close: () => Promise<void>;
              }>;
            }>;
          }
        ).showSaveFilePicker;
        if (picker) {
          const handle = await picker({
            suggestedName: `movie-party-diagnostics-${new Date().toISOString().slice(0, 10)}.json`,
          });
          const writable = await handle.createWritable();
          await writable.write(new Blob([diagnosticBundle], { type: "application/json" }));
          await writable.close();
          setExportedBundle("Saved.");
        } else {
          await navigator.clipboard.writeText(diagnosticBundle);
          setExportedBundle("Copied to clipboard.");
        }
      } catch {
        setExportedBundle("Export cancelled or unavailable.");
      }
      exportTimer.current = window.setTimeout(() => {
        setExportedBundle(null);
      }, 2400);
    })();
  };

  return (
    <div
      className="relative w-full min-h-screen flex flex-col"
      data-testid="settings"
    >
      <div className="fixed inset-0 pointer-events-none">
        <SilkBackground variant="calm" />
      </div>
      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <button
          type="button"
          onClick={onBack}
          className="flex items-center gap-2 text-white/60 hover:text-white transition text-sm tracking-wider"
          data-testid="settings-back-btn"
        >
          <ArrowLeft className="w-4 h-4" strokeWidth={1.6} /> Back
        </button>
        <StatusIndicator state="sync" label="Strict Sync" />
      </header>

      <main className="relative z-10 max-w-[1300px] w-full mx-auto px-12 mt-6 pb-20 flex-1 settings-layout">
        <nav aria-label="Settings sections" className="settings-nav">
          <h1 className="font-serif-display text-white text-4xl tracking-[-0.02em]">Settings</h1>
          <div className="mt-8 rounded-2xl border border-white/10 bg-white/[0.03] p-2.5">
            <ul className="space-y-1">
            {SECTIONS.map((item) => (
              <li key={item.id}>
                <button
                  type="button"
                  onClick={() => {
                    setSection(item.id);
                  }}
                  aria-current={section === item.id}
                  className={`w-full text-left px-4 py-2.5 rounded-lg text-sm transition ${
                    section === item.id
                      ? "bg-white/[0.09] text-white shadow-[inset_2px_0_0_0_#9F7AEA]"
                      : "text-white/55 hover:text-white hover:bg-white/[0.04]"
                  }`}
                >
                  {item.label}
                </button>
              </li>
            ))}
          </ul>
          </div>
        </nav>

        {/* Vertical partition between the nav and the content pane. */}
        <div className="settings-divider" aria-hidden="true" />

        <motion.section
          key={section}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.25 }}
          className="settings-content max-w-2xl"
        >
          <h2
            className="font-serif-display text-2xl text-white/90 tracking-tight"
            data-testid={`settings-section-title-${section}`}
          >
            {SECTIONS.find((item) => item.id === section)?.label ?? "Settings"}
          </h2>
          <div className="mt-5 rounded-2xl border border-white/10 bg-white/[0.03] px-6 py-5">
          {section === "general" ? (
            <div data-testid="settings-general">
              <Row
                label="Display name"
                hint={
                  nameError ??
                  (nameSaving
                    ? "Saving…"
                    : "Your name is shown to your movie partner. 1–40 characters.")
                }
              >
                <form
                  onSubmit={(event) => {
                    event.preventDefault();
                    saveName();
                  }}
                  className="flex items-center justify-end gap-2"
                >
                  <input
                    type="text"
                    value={nameDraft}
                    maxLength={40}
                    aria-label="Display name"
                    aria-invalid={nameError != null}
                    data-testid="settings-display-name-input"
                    onChange={(event) => {
                      setNameDraft(event.target.value);
                    }}
                    onBlur={saveName}
                    onKeyDown={(event) => {
                      if (event.key === "Escape") {
                        setNameDraft(
                          snapshot.participants[0]?.displayName ?? "You",
                        );
                        setNameError(null);
                      }
                    }}
                    className="settings-name-input font-mono-mp"
                  />
                  <button
                    type="submit"
                    disabled={nameSaving}
                    data-testid="settings-display-name-save"
                    aria-label="Save display name"
                    className="settings-name-save"
                  >
                    Save
                  </button>
                </form>
              </Row>
              <Row label="Start Movie Party at login" hint="V1: off by default">
                Off
              </Row>
              <Row label="Notification preferences" hint="Reminders for scheduled preloads">
                System-managed
              </Row>
              <div className="pt-4 border-t border-white/[0.06]" data-testid="settings-tmdb">
                <Row
                  label="Trending posters (TMDB)"
                  hint={
                    !tmdbTokenSaved
                      ? bundledTmdbToken().length > 0
                        ? "A shared key ships with this build, so the Home hero fetches trending posters out of the box. Paste your own TMDB key (v3) or read access token only to override it on this device — for example if the shared key stops working."
                        : "Optional: paste your own TMDB API key (v3) or read access token to light the Home hero with trending posters. Without it, the built-in gradient wall shows. The key is stored on this device only."
                      : tmdbLiveOk
                        ? "Live feed working — the Home hero is showing TMDB's trending week."
                        : tmdbStatus?.lastError === "network"
                          ? "Your key is saved, but this device could not reach TMDB (network blocked/unreachable). The built-in wall shows in the meantime; posters appear once TMDB is reachable."
                          : "Your key is saved. The Home hero fetches trending posters when it can; the built-in wall shows otherwise."
                  }
                >
                  <button
                    type="button"
                    onClick={() => {
                      setTmdbDialogOpen(true);
                    }}
                    className="px-4 py-2 rounded-full text-[11px] tracking-[0.12em] uppercase border border-white/15 text-white/55 hover:text-white hover:border-white/30 transition"
                    data-testid="settings-tmdb-edit-btn"
                  >
                    {tmdbTokenSaved ? "Change key" : "Add key"}
                  </button>
                </Row>
                {tmdbTokenSaved ? (
                  <div className="flex items-center gap-3 pb-3">
                    <button
                      type="button"
                      onClick={() => {
                        clearTmdbToken();
                      }}
                      className="px-4 py-2 rounded-full text-[11px] tracking-[0.12em] uppercase border border-white/15 text-white/55 hover:text-white transition"
                      data-testid="settings-tmdb-clear-btn"
                    >
                      Remove key
                    </button>
                    <span className="text-xs text-white/40">
                      Falls back to the built-in wall — the app never requires TMDB.
                    </span>
                  </div>
                ) : null}
              </div>
            </div>
          ) : null}

          {section === "playback" ? (
            <div data-testid="settings-playback">
              <Row label="Subtitle preference" hint="Chosen per-title in the player" />
              <Row label="Preferred audio track" hint="Chosen per-title in the player" />
              <Row label="Default volume">
                {`${String(Math.round(snapshot.player.volume * 100))}%`}
              </Row>
              <Row label="10-second seek amount" hint="← / → in Cinema Mode">
                Enabled
              </Row>
              <Row
                label="Strict Sync"
                hint="Playback pauses for BOTH seats when either can't continue — this is the product (§56)"
              >
                <span className="text-sky-200/80">Always on</span>
              </Row>
            </div>
          ) : null}

          {section === "network" ? (
            <div data-testid="settings-network">
              <Row label="Tailscale status">
                {tailscale ? tailscale.message : "Checking…"}
              </Row>
              <Row label="Transport">{snapshot.network.transport}</Row>
              <Row label="Path" hint="Advanced — shown in Diagnostics too">
                <span className="text-white/50 text-xs">{snapshot.network.path}</span>
              </Row>
              <Row label="Connection diagnostics" hint="Host measures RTT on the live session">
                {snapshot.network.rttMs != null
                  ? `${String(snapshot.network.rttMs)} ms RTT`
                  : "—"}
              </Row>
            </div>
          ) : null}

          {section === "call" ? (
            <div data-testid="settings-call">
              <Row label="Camera device" hint="System default camera">
                {snapshot.call.camera.enabled
                  ? `${String(snapshot.call.camera.width)}p`
                  : "Disabled"}
              </Row>
              <Row label="Microphone device" hint="System default microphone">
                {snapshot.call.microphone.enabled ? "Active" : "Muted"}
              </Row>
              <Row label="Mirror self preview" hint="PiP view mirrors you, not the movie">
                On
              </Row>
              <Row
                label="Noise suppression"
                hint="Uses the browser's built-in track processing"
              >
                Automatic
              </Row>
            </div>
          ) : null}

          {section === "storage" ? (
            <div data-testid="settings-storage">
              <Row label="Cache location" hint="Managed by Movie Party; never the source file">
                App cache directory
              </Row>
              <Row label="Cache used">
                {snapshot.transfer
                  ? formatBytes(snapshot.transfer.bytesAvailable)
                  : "Nothing cached yet"}
              </Row>
              <Row
                label="Clear cache"
                hint="Removes Movie Party's downloaded copy — never your original movie"
              >
                <CinemaButton variant="neutral" icon={Trash2} iconPos="left">
                  Clear
                </CinemaButton>
              </Row>
              <Row label="Post-party media policy" hint="Keep / Remove is asked per party (§52)">
                Ask every time
              </Row>
            </div>
          ) : null}

          {section === "providers" ? (
            <div data-testid="settings-providers" className="space-y-4">
              {capabilities.map((capability) => {
                const providerSnapshot =
                  snapshot.provider.providerId === capability.id ? snapshot.provider : null;
                const resetting = confirmResetProvider === capability.id;
                return (
                  <article
                    key={capability.id}
                    className="rounded-xl border border-white/10 bg-white/[0.03] p-5"
                    data-testid={`provider-card-${capability.id}`}
                  >
                    <header className="flex items-center justify-between">
                      <h3 className="text-base text-white/90">{capability.displayName}</h3>
                      <span
                        className={`text-[11px] tracking-[0.14em] uppercase px-2.5 py-1 rounded-full border ${
                          capability.supportLevel === "SUPPORTED"
                            ? "border-emerald-400/30 text-emerald-200/90"
                            : "border-amber-400/30 text-amber-200/90"
                        }`}
                      >
                        {capability.supportLevel === "SUPPORTED"
                          ? "Supported"
                          : capability.supportLevel === "PARTIAL"
                            ? "Partial"
                            : "Unavailable"}
                      </span>
                    </header>
                    <dl className="mt-3 text-sm text-white/60 space-y-1.5">
                      <div className="flex gap-3">
                        <dt className="w-24 shrink-0 text-white/40">Session</dt>
                        <dd>
                          {providerSnapshot
                            ? providerSnapshot.readiness === "PLAYBACK_READY"
                              ? "Signed in"
                              : providerSnapshot.readiness === "LOGIN_REQUIRED"
                                ? "Login required"
                                : "Unknown"
                            : "Unknown"}
                        </dd>
                      </div>
                      <div className="flex gap-3">
                        <dt className="w-24 shrink-0 text-white/40">Shared Mode</dt>
                        <dd data-testid={`shared-status-${capability.id}`}>
                          {(() => {
                            const record = diagnostics[capability.id];
                            if (record == null) {
                              return capability.sharedReason || "Experimental / Unsupported";
                            }
                            if (record.sharedAvailable) {
                              return "Verified on this device (experimental)";
                            }
                            return record.sharedReason || "Unavailable on this device";
                          })()}
                        </dd>
                      </div>
                    </dl>
                    {diagnosticMessage != null && (diagnosticRunning == null) ? (
                      <div
                        className="mt-3 rounded-lg border border-white/10 bg-white/[0.03] px-3.5 py-2.5 text-xs text-white/70"
                        role="status"
                        data-testid="shared-diagnostic-message"
                      >
                        {diagnosticMessage}
                        {diagnosticMessage.includes("Sync Mode") ? (
                          <span className="block mt-1.5 text-white/45">
                            Provider Sync Mode is the supported path for this provider —
                            both of you sign in on your own devices.
                          </span>
                        ) : null}
                      </div>
                    ) : null}
                    <div className="mt-4 flex flex-wrap gap-3">
                      <CinemaButton
                        variant="neutral"
                        onClick={() => {
                          void (async () => {
                            await openProviderBrowser(capability.id);
                          })();
                        }}
                      >
                        Open Provider
                      </CinemaButton>
                      <CinemaButton
                        variant="neutral"
                        disabled={diagnosticRunning != null}
                        onClick={() => {
                          runDiagnostic(capability.id);
                        }}
                      >
                        {diagnosticRunning === capability.id
                          ? "Running diagnostic…"
                          : "Run Shared Diagnostic"}
                      </CinemaButton>
                      <CinemaButton
                        variant="neutral"
                        icon={RotateCcw}
                        iconPos="left"
                        onClick={() => {
                          if (!resetting) {
                            setConfirmResetProvider(capability.id);
                            return;
                          }
                          // §60: reset requires confirmation because it logs the user out.
                          setConfirmResetProvider(null);
                          void (async () => {
                            await checkProviderStatus(capability.id);
                          })();
                        }}
                      >
                        {resetting ? "Confirm reset — logs you out" : "Reset Provider Profile"}
                      </CinemaButton>
                    </div>
                  </article>
                );
              })}
            </div>
          ) : null}

          {section === "privacy" ? (
            <div data-testid="settings-privacy">
              <Row label="Ghost Mode" hint="Ctrl/Cmd + Shift + M — hides social surfaces">
                {snapshot.ghostMode ? "Active" : "Off"}
              </Row>
              <Row label="Privacy Mode" hint="Ctrl/Cmd + Shift + P — camera/mic off for both">
                {snapshot.privacyMode ? "Active" : "Off"}
              </Row>
              <Row label="Chat retention" hint="Chats live only for this party (§34–36)">
                Per-party
              </Row>
              <Row label="Diagnostic log retention" hint="Technical details stay on this device">
                Local only
              </Row>
              <Row
                label="Where your data lives"
                hint="Media, chats, schedules, and diagnostics never leave your devices"
              >
                Peer-to-peer, local-first
              </Row>
              <Row
                label="Provider credentials"
                hint="Sign-ins stay inside each device's dedicated Chrome profile"
              >
                Never stored by Movie Party
              </Row>
              <Row
                label="Telemetry"
                hint="No usage data is collected, transmitted, or sold"
              >
                None
              </Row>
            </div>
          ) : null}

          {section === "diagnostics" ? (
            <div data-testid="settings-diagnostics">
              <Row label="App version">{metadata.appVersion}</Row>
              <Row label="Protocol version" hint="Wire compatibility (ADR-0001)">
                {metadata.protocol}
              </Row>
              <Row label="Tailscale path">
                <span className="text-white/50 text-xs">{snapshot.network.path}</span>
              </Row>
              <Row label="Peer RTT">
                {snapshot.network.rttMs != null
                  ? `${String(snapshot.network.rttMs)} ms`
                  : "—"}
              </Row>
              <Row label="Debug HUD" hint="Development only — see ?debug in dev preview">
                Dev-gated
              </Row>
              <Row label="Export diagnostic bundle" hint="Saved locally as JSON">
                <span className="flex items-center gap-3">
                  {exportedBundle ? (
                    <span className="text-xs text-emerald-300/80">{exportedBundle}</span>
                  ) : null}
                  <CinemaButton
                    variant="neutral"
                    icon={Download}
                    iconPos="left"
                    onClick={() => {
                      exportBundle();
                    }}
                  >
                    Export
                  </CinemaButton>
                </span>
              </Row>
            </div>
          ) : null}
          </div>
        </motion.section>
      </main>

      {tmdbDialogOpen ? (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center p-6 bg-black/45 backdrop-blur-[2px]"
          role="dialog"
          aria-label="TMDB key"
          data-testid="settings-tmdb-dialog"
        >
          <div className="relative w-[min(560px,92vw)] rounded-2xl bg-[#0D0B14]/95 border border-white/10 shadow-2xl p-6">
            <h3 className="font-serif-display text-xl text-white">Trending posters (TMDB)</h3>
            <p className="mt-3 text-sm text-white/55 leading-relaxed">
              {bundledTmdbToken().length > 0
                ? "This build already ships a shared key, so posters work out of the box. Paste a key ONLY to override the shared one on this device — e.g. if it stops working. Create a free one at themoviedb.org → Settings → API. Stored on this device only; never included in invites or sync data."
                : "Paste your own TMDB API key (v3) or read access token. Create a free one at themoviedb.org → Settings → API. It is stored on this device only and never included in invites or sync data."}
            </p>
            <input
              type="password"
              value={tmdbDraft}
              onChange={(event) => {
                setTmdbDraft(event.target.value);
              }}
              placeholder="Your TMDB key or read access token"
              className="mt-4 w-full bg-[#05050B] border border-white/15 rounded-lg px-3.5 py-2.5 text-sm text-white/95 placeholder:text-white/40 focus:outline-none focus:border-sky-400/50 font-mono-mp"
              data-testid="settings-tmdb-input"
              aria-label="TMDB key"
            />
            <p className="mt-3 text-xs text-white/40 leading-relaxed">
              The Home hero shows trending titles with the required TMDB attribution. If TMDB is
              unreachable, the built-in poster wall shows instead — the app never depends on it.
            </p>
            <div className="mt-5 flex items-center justify-end gap-3">
              <button
                type="button"
                onClick={() => {
                  setTmdbDialogOpen(false);
                  setTmdbDraft("");
                }}
                className="px-4 py-2.5 rounded-full text-xs tracking-[0.12em] uppercase text-white/55 hover:text-white transition"
                data-testid="settings-tmdb-cancel-btn"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={tmdbDraft.trim().length === 0}
                onClick={() => {
                  saveTmdbToken();
                }}
                className="px-5 py-2.5 rounded-full text-xs tracking-[0.12em] uppercase border border-[#9F7AEA]/50 bg-[#6B46C1]/30 text-white disabled:opacity-40 disabled:cursor-not-allowed transition"
                data-testid="settings-tmdb-save-btn"
              >
                Save key
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
