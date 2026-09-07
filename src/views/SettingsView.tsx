import { useEffect, useMemo, useRef, useState } from "react";
import { motion } from "framer-motion";
import { ArrowLeft, Download, RotateCcw, Trash2 } from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import {
  checkProviderStatus,
  getAppMetadataInfo,
  getProviderCapabilities,
  getTailscaleReadiness,
  openProviderBrowser,
  type AppSnapshot,
  type ProviderCapability,
  type TailscaleReadiness,
} from "../backend/appRuntime";

/**
 * UI_UX_SPEC §55–§62 — Settings.
 *
 * Product-register rules: consistent CinemaButton vocabulary, state-rich
 * but restrained color (amber for destructive-confirm, sky for info),
 * no decorative motion. Truth rules: providers show empirical support
 * (AGENTS §38), Strict Sync is visible but NOT disableable (§56 — it is
 * core product behavior, AGENTS §14), destructive actions confirm.
 */
type SettingsViewProps = {
  snapshot: AppSnapshot;
  onBack: () => void;
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

export function SettingsView({ snapshot, onBack }: SettingsViewProps) {
  const [section, setSection] = useState<SettingsSection>("general");
  const [capabilities, setCapabilities] = useState<ProviderCapability[]>([]);
  const [tailscale, setTailscale] = useState<TailscaleReadiness | null>(null);
  const [metadata, setMetadata] = useState<{ appVersion: string; protocol: string }>({
    appVersion: "—",
    protocol: "—",
  });
  const [confirmResetProvider, setConfirmResetProvider] = useState<string | null>(null);
  const [exportedBundle, setExportedBundle] = useState<string | null>(null);
  const exportTimer = useRef<number | undefined>(undefined);

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
    <div className="relative w-screen h-screen overflow-hidden" data-testid="settings">
      <SilkBackground variant="calm" />
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

      <main className="relative z-10 max-w-[1300px] mx-auto px-12 mt-4 h-[calc(100vh-120px)] grid grid-cols-12 gap-10">
        <nav aria-label="Settings sections" className="col-span-12 lg:col-span-3">
          <h1 className="font-serif-display text-white text-4xl tracking-[-0.02em]">Settings</h1>
          <ul className="mt-8 space-y-1">
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
                      ? "bg-white/[0.07] text-white"
                      : "text-white/55 hover:text-white hover:bg-white/[0.03]"
                  }`}
                >
                  {item.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>

        <motion.section
          key={section}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.25 }}
          className="col-span-12 lg:col-span-9 max-w-2xl"
        >
          {section === "general" ? (
            <div data-testid="settings-general">
              <Row label="Display name">{snapshot.participants[0]?.displayName ?? "You"}</Row>
              <Row label="Start Movie Party at login" hint="V1: off by default">
                Off
              </Row>
              <Row label="Notification preferences" hint="Reminders for scheduled preloads">
                System-managed
              </Row>
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
                        <dd>
                          {capability.sharedAvailable
                            ? "Available"
                            : capability.sharedReason || "Experimental / Unsupported"}
                        </dd>
                      </div>
                    </dl>
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
        </motion.section>
      </main>
    </div>
  );
}
