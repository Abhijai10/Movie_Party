import { AnimatePresence, motion } from "framer-motion";
import {
  AlertCircle,
  ArrowLeft,
  ArrowRight,
  CheckCircle2,
  Film,
  Link2,
  Loader2,
  Play,
  Upload,
  X,
} from "lucide-react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useCallback, useEffect, useState } from "react";
import {
  pickMediaFile,
  type ProviderCapability,
  type ProviderMode,
  type ProviderReadiness,
} from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { SourceCard } from "../components/mp/SourceCard";
import { isGenericLink } from "../providers/providerSelection";
import { ProviderContentChooser } from "./ProviderContentChooser";
import {
  droppedPathErrorMessage,
  mediaFileNameFromPath,
  usableDroppedPath,
} from "./createPartySource";

type SourceKind = "local" | "stream" | "link";

export type CreatePartyRequest =
  | { source: "local"; mediaPath: string | null }
  | {
      source: "provider";
      providerId: string;
      url: string;
      mode: ProviderMode;
      selectionKey: string;
    }
  | { source: "link"; url: string };

export type ProviderStatus = {
  providerId: string | null;
  url: string | null;
  state: string;
  readiness: ProviderReadiness;
};

type CreatePartyViewProps = {
  isCreating: boolean;
  error: string | null;
  providerCapabilities: ProviderCapability[];
  provider: ProviderStatus;
  onBack: () => void;
  onCreateParty: (request: CreatePartyRequest) => Promise<boolean>;
  onOpenProvider: (providerId: string) => Promise<boolean>;
  onCheckProviderStatus: (providerId: string) => Promise<boolean>;
  onNavigateProviderTitle: (providerId: string, title: string) => Promise<boolean>;
};

const sources = [
  { key: "local", icon: Film, title: "Local movie", desc: "A downloaded film on this device." },
  {
    key: "stream",
    icon: Play,
    title: "Streaming provider",
    desc: "A supported provider from your own profile.",
  },
  {
    key: "link",
    icon: Link2,
    title: "Third-party link",
    desc: "Paste a direct URL for a compatible source.",
  },
] as const;

export function CreatePartyView({
  isCreating,
  error,
  providerCapabilities,
  onBack,
  onCreateParty,
  onOpenProvider,
}: CreatePartyViewProps) {
  const [selected, setSelected] = useState<SourceKind>("local");
  const [dragOver, setDragOver] = useState(false);
  const [path, setPath] = useState("");
  const [providerId, setProviderId] = useState("");
  const [status, setStatus] = useState<"idle" | "error">("idle");
  /** Honest reason a local source was refused (F3). */
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [preparing, setPreparing] = useState(false);

  const pick = (key: SourceKind) => {
    setSelected(key);
    setStatus("idle");
    setSourceError(null);
    setPath("");
    setPreparing(false);
  };

  useEffect(() => {
    if (!providerId && providerCapabilities[0]) {
      setProviderId(providerCapabilities[0].id);
    }
  }, [providerCapabilities, providerId]);

  /**
   * Accept a dropped selection.
   *
   * Only a real filesystem path counts, because the backend is handed
   * `mediaPath` and nothing else. A browser `File` carries no path, so
   * treating one as a selection is what produced a cinema room with no
   * media (F3). When a drop cannot be used we say so rather than showing a
   * selection that can never play.
   */
  const applyDroppedPaths = useCallback((paths: readonly string[]) => {
    const usable = usableDroppedPath(paths);
    if (usable == null) {
      setStatus("error");
      setSourceError(droppedPathErrorMessage(paths));
      return;
    }
    setPath(usable);
    setSourceError(null);
    setStatus("idle");
  }, []);

  // Native drag-and-drop is the platform mechanism that yields real paths.
  // Tauri intercepts file drops at the window level, so the HTML5 handlers
  // on the dropzone only drive the hover affordance and report an unusable
  // drop; the path always comes from here.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter" || payload.type === "over") {
          setDragOver(true);
          return;
        }
        if (payload.type === "leave") {
          setDragOver(false);
          return;
        }
        setDragOver(false);
        applyDroppedPaths(payload.paths);
      })
      .then((next) => {
        if (disposed) {
          next();
          return;
        }
        unlisten = next;
      })
      .catch(() => {
        // No native drag-drop (browser dev): the HTML5 handlers still run.
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applyDroppedPaths]);

  const activeSource = path.trim();
  const hasSelection = Boolean(activeSource);
  const selectedMovieName = mediaFileNameFromPath(path);

  const prepare = () => {
    if (!hasSelection) {
      setStatus("error");
      return;
    }
    if (selected === "link" && !isGenericLink(activeSource)) {
      setStatus("error");
      return;
    }
    setStatus("idle");
    setPreparing(true);
  };

  const create = () => {
    if (selected === "link") {
      void onCreateParty({ source: "link", url: activeSource });
      return;
    }
    void onCreateParty({ source: "local", mediaPath: activeSource || null });
  };

  return (
    <div className="relative w-screen h-screen overflow-hidden">
      <SilkBackground variant="calm" />

      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <button
          type="button"
          onClick={onBack}
          className="flex items-center gap-2 text-white/60 hover:text-white transition text-sm tracking-wider"
          data-testid="create-back-btn"
        >
          <ArrowLeft className="w-4 h-4" strokeWidth={1.6} /> Back
        </button>
        <span className="text-[11px] tracking-[0.28em] uppercase text-white/50">
          Step 01 · Prepare the room
        </span>
      </header>

      <main
        className={`relative z-10 max-w-[1300px] mx-auto px-12 mt-10 grid grid-cols-12 gap-10 h-[calc(100vh-130px)] ${selected === "stream" ? "provider-create-layout" : ""}`}
      >
        <motion.section
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, ease: "easeOut" }}
          className={`col-span-12 ${selected === "stream" ? "provider-create-sources" : "lg:col-span-7"} flex flex-col`}
        >
          <h1 className="font-serif-display text-white text-[56px] leading-[0.98] tracking-tight">
            What are we <span className="italic">watching</span> tonight?
          </h1>
          <p className="mt-5 text-white/55 text-base max-w-lg leading-relaxed">
            Choose a source, prepare the room, and send the invitation when everything is ready.
          </p>

          <div
            className={`mt-10 grid ${selected === "stream" ? "grid-cols-1" : "grid-cols-3"} gap-4`}
          >
            {sources.map((s) => (
              <SourceCard
                key={s.key}
                icon={s.icon}
                title={s.title}
                description={s.desc}
                active={selected === s.key}
                onClick={() => {
                  pick(s.key);
                }}
                testId={`source-${s.key}`}
              />
            ))}
          </div>

          {/*
            Errors live HERE, directly under the source cards, rather than at
            the bottom of the right-hand panel. The old placement was below the
            fold: at the default window size a failed "Create party" produced a
            message the user could not see at all — it only appeared in
            fullscreen. A failure that silently looks like a no-op is worse than
            the failure itself.
          */}
          {(status === "error" || error) && (
            <div
              role="alert"
              aria-live="assertive"
              className="mt-6 flex items-start gap-2.5 rounded-xl border border-[#F87171]/30 bg-[#F87171]/[0.06] px-4 py-3 text-[#FCA5A5] text-sm leading-relaxed"
              data-testid="create-error"
            >
              <AlertCircle className="w-4 h-4 mt-0.5 shrink-0" />
              <span>
                {error ??
                  sourceError ??
                  (selected === "stream"
                    ? "Choose an available provider, a supported mode, and its matching page URL."
                    : "Unsupported source. Try MP4, MKV or a direct URL.")}
              </span>
            </div>
          )}
        </motion.section>

        <motion.section
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, ease: "easeOut", delay: 0.15 }}
          className={`col-span-12 ${selected === "stream" ? "provider-create-panel" : "lg:col-span-5"} flex flex-col min-h-0`}
        >
          <span className="text-[11px] tracking-[0.28em] uppercase text-white/50 mb-4">
            Selection
          </span>

          <AnimatePresence mode="wait">
            {selected === "local" && (
              <motion.div
                key="local"
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -10 }}
                transition={{ duration: 0.3 }}
                className="flex-1 flex flex-col"
              >
                <div
                  onDragOver={(e) => {
                    e.preventDefault();
                    setDragOver(true);
                  }}
                  onDragLeave={() => {
                    setDragOver(false);
                  }}
                  onDrop={(e) => {
                    e.preventDefault();
                    setDragOver(false);
                    // The HTML5 drop carries no filesystem path, so it can
                    // never satisfy the backend's mediaPath. The native
                    // drag-drop listener above is the one that accepts a
                    // drop; reaching here means the file genuinely cannot
                    // be opened, so say that instead of faking a selection.
                    applyDroppedPaths([]);
                  }}
                  onClick={() => {
                    void pickMediaFile().then((picked) => {
                      if (picked) {
                        setPath(picked);
                        setSourceError(null);
                        setStatus("idle");
                      }
                    });
                  }}
                  className={`relative flex-1 rounded-2xl cursor-pointer flex flex-col items-center justify-center gap-3 transition ${
                    dragOver
                      ? "border border-[#9F7AEA] bg-[#150F24]"
                      : "border border-dashed border-white/15 bg-white/[0.02] hover:border-[#9F7AEA]/40"
                  }`}
                  data-testid="upload-dropzone"
                >
                  {!activeSource ? (
                    <>
                      <div
                        className="w-14 h-14 rounded-full flex items-center justify-center"
                        style={{
                          background: "rgba(159,122,234,0.12)",
                          border: "1px solid rgba(159,122,234,0.3)",
                        }}
                      >
                        <Upload className="w-5 h-5 text-white/85" strokeWidth={1.5} />
                      </div>
                      <div className="text-center">
                        <p className="font-serif-display text-2xl text-white">Select movie</p>
                        <p className="text-white/45 text-xs tracking-[0.2em] uppercase mt-2">
                          Click to browse · MP4 · MKV · MOV
                        </p>
                      </div>
                    </>
                  ) : (
                    <div className="w-full px-8 text-center">
                      <div className="flex items-center justify-center gap-2 text-[#34D399] text-[11px] tracking-[0.24em] uppercase mb-3">
                        <CheckCircle2 className="w-3.5 h-3.5" /> Selected
                      </div>
                      <p className="font-serif-display text-2xl text-white truncate">
                        {selectedMovieName}
                      </p>
                      <p className="text-white/45 text-xs mt-2">
                        Local playback is ready to prepare.
                      </p>
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          setPath("");
                          setSourceError(null);
                          setStatus("idle");
                        }}
                        className="mt-4 inline-flex items-center gap-1.5 text-white/60 hover:text-white text-xs"
                        data-testid="upload-clear-btn"
                      >
                        <X className="w-3 h-3" /> Clear
                      </button>
                    </div>
                  )}
                </div>

                <div className="mt-5">
                  <label className="text-[11px] tracking-[0.24em] uppercase text-white/45">
                    Or paste a local path
                  </label>
                  <input
                    type="text"
                    value={path}
                    onChange={(e) => {
                      setPath(e.target.value);
                      setSourceError(null);
                      setStatus("idle");
                    }}
                    placeholder="/Users/you/Movies/Inception.mkv"
                    className="mt-2 w-full bg-white/5 border border-white/10 rounded-xl px-4 py-3 text-sm text-white placeholder:text-white/30 focus:outline-none focus:border-[#9F7AEA]/50 transition font-mono-mp"
                    data-testid="local-path-input"
                  />
                </div>
              </motion.div>
            )}

            {selected === "stream" && (
              <ProviderContentChooser
                key={providerId}
                capabilities={providerCapabilities}
                providerId={providerId}
                onProviderChange={setProviderId}
                onOpenProvider={onOpenProvider}
                onCreateParty={onCreateParty}
                isCreating={isCreating}
              />
            )}

            {selected === "link" && (
              <motion.div
                key="link"
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -10 }}
                transition={{ duration: 0.3 }}
                className="flex-1 flex flex-col"
              >
                <div className="rounded-2xl border border-white/10 bg-white/[0.02] p-6 flex-1 flex flex-col justify-center">
                  <label className="text-[11px] tracking-[0.24em] uppercase text-white/45">
                    Direct URL
                  </label>
                  <input
                    type="url"
                    value={path}
                    onChange={(e) => {
                      setPath(e.target.value);
                      setStatus("idle");
                    }}
                    placeholder="https://example.com/movie.mp4"
                    className="mt-3 w-full bg-transparent border-b border-white/15 pb-3 text-white text-lg focus:outline-none focus:border-[#9F7AEA]/60 transition font-mono-mp"
                    data-testid="link-url-input"
                  />
                  <p className="mt-5 text-white/40 text-xs leading-relaxed">
                    Supports direct-play formats. HLS (.m3u8) and MP4 preferred.
                  </p>
                </div>
              </motion.div>
            )}
          </AnimatePresence>

          {selected !== "stream" && (
            <>
              {preparing ? (
                <motion.div
                  initial={{ opacity: 0, y: 8 }}
                  animate={{ opacity: 1, y: 0 }}
                  className="mt-6 rounded-2xl p-5"
                  style={{
                    background: "rgba(13, 11, 20, 0.7)",
                    border: "1px solid rgba(159,122,234,0.18)",
                    backdropFilter: "blur(14px)",
                  }}
                >
                  <div className="flex items-center gap-4">
                    <div
                      className="w-12 h-12 rounded-xl flex items-center justify-center"
                      style={{
                        background: "linear-gradient(135deg, #6B46C1, #3D2168)",
                        border: "1px solid rgba(159,122,234,0.25)",
                      }}
                    >
                      <Film className="w-5 h-5 text-white" strokeWidth={1.6} />
                    </div>
                    <div className="min-w-0">
                      <span className="text-[10px] tracking-[0.24em] uppercase text-white/45">
                        Selected source
                      </span>
                      <p className="font-serif-display text-xl text-white truncate">
                        {selectedMovieName || "Streaming session"}
                      </p>
                    </div>
                  </div>
                  <div className="mt-4 grid gap-4 min-w-0">
                    <span className="text-[11px] tracking-[0.24em] uppercase text-white/40 truncate">
                      Your screening is ready to open
                    </span>
                    {/* Same size language as the rest of the app: the two
                        actions sit on one row, each at its natural single
                        -line width — never stretched by a narrow grid
                        column into a tall wrapped pill. */}
                    <div className="flex flex-wrap items-center justify-center gap-3 min-w-0">
                      <CinemaButton
                        variant="ghost"
                        disabled={isCreating}
                        onClick={() => {
                          setPreparing(false);
                        }}
                      >
                        Change
                      </CinemaButton>
                      <CinemaButton
                        onClick={create}
                        disabled={isCreating}
                        icon={isCreating ? Loader2 : ArrowRight}
                        className={isCreating ? "[&_svg]:animate-spin" : ""}
                        data-testid="create-room-btn"
                      >
                        {isCreating
                          ? "Creating"
                          : selected === "link"
                            ? "Open link room"
                            : "Create cinema room"}
                      </CinemaButton>
                    </div>
                  </div>
                </motion.div>
              ) : (
                <div className="mt-6 flex items-center justify-between">
                  <span className="text-[11px] tracking-[0.24em] uppercase text-white/40">
                    A room code will be generated when ready
                  </span>
                  <CinemaButton
                    onClick={prepare}
                    disabled={!hasSelection}
                    icon={ArrowRight}
                    data-testid="prepare-cinema-btn"
                  >
                    {selected === "link" ? "Prepare link" : "Prepare Cinema"}
                  </CinemaButton>
                </div>
              )}
            </>
          )}
        </motion.section>
      </main>
    </div>
  );
}
