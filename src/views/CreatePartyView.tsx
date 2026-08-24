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
import { useState } from "react";
import { pickMediaFile } from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
import { SilkBackground } from "../components/mp/SilkBackground";
import { SourceCard } from "../components/mp/SourceCard";

type SourceKind = "local" | "stream" | "link";

type CreatePartyViewProps = {
  isCreating: boolean;
  error: string | null;
  onBack: () => void;
  onCreateLocalParty: (mediaPath: string | null) => Promise<boolean>;
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
  onBack,
  onCreateLocalParty,
}: CreatePartyViewProps) {
  const [selected, setSelected] = useState<SourceKind>("local");
  const [file, setFile] = useState<File | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [path, setPath] = useState("");
  const [status, setStatus] = useState<"idle" | "error">("idle");
  const [preparing, setPreparing] = useState(false);

  const pick = (key: SourceKind) => {
    setSelected(key);
    setStatus("idle");
    setFile(null);
    setPath("");
    setPreparing(false);
  };

  const onFile = (f: File | null) => {
    if (!f) return;
    const okExt = /\.(mp4|mkv|mov|webm)$/i.test(f.name);
    if (!okExt) {
      setStatus("error");
      return;
    }
    setFile(f);
    setStatus("idle");
  };

  const activeSource = selected === "local" ? path.trim() : path.trim();
  const hasSelection = selected === "local" ? Boolean(file || activeSource) : Boolean(activeSource);
  const selectedMovieName =
    file?.name ?? (path.trim() ? (path.trim().split(/[\\/]/).at(-1) ?? "") : "");

  const prepare = () => {
    if (!hasSelection) {
      setStatus("error");
      return;
    }
    setStatus("idle");
    setPreparing(true);
  };

  const create = () => {
    void onCreateLocalParty(activeSource || null);
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

      <main className="relative z-10 max-w-[1300px] mx-auto px-12 mt-10 grid grid-cols-12 gap-10 h-[calc(100vh-130px)]">
        <motion.section
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, ease: "easeOut" }}
          className="col-span-12 lg:col-span-7 flex flex-col"
        >
          <h1 className="font-serif-display text-white text-[56px] leading-[0.98] tracking-tight">
            What are we <span className="italic">watching</span> tonight?
          </h1>
          <p className="mt-5 text-white/55 text-base max-w-lg leading-relaxed">
            Choose a source, prepare the room, and send the invitation when everything is ready.
          </p>

          <div className="mt-10 grid grid-cols-3 gap-4">
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
        </motion.section>

        <motion.section
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, ease: "easeOut", delay: 0.15 }}
          className="col-span-12 lg:col-span-5 flex flex-col"
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
                    onFile(e.dataTransfer.files[0] ?? null);
                  }}
                  onClick={() => {
                    if (file) return;
                    void pickMediaFile().then((picked) => {
                      if (picked) {
                        setPath(picked);
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
                  {!file && !path ? (
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
                        {file
                          ? `${(file.size / (1024 * 1024)).toFixed(1)} MB`
                          : "Local playback is ready to prepare."}
                      </p>
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          setFile(null);
                          setPath("");
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
              <motion.div
                key="stream"
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -10 }}
                transition={{ duration: 0.3 }}
                className="flex-1 flex flex-col"
              >
                <div className="rounded-2xl border border-white/10 bg-white/[0.02] p-6 flex-1 flex flex-col justify-center">
                  <label className="text-[11px] tracking-[0.24em] uppercase text-white/45">
                    Provider URL
                  </label>
                  <input
                    type="url"
                    value={path}
                    onChange={(e) => {
                      setPath(e.target.value);
                      setStatus("idle");
                    }}
                    placeholder="https://www.netflix.com/watch/..."
                    className="mt-3 w-full bg-transparent border-b border-white/15 pb-3 text-white text-lg focus:outline-none focus:border-[#9F7AEA]/60 transition font-mono-mp"
                    data-testid="provider-url-input"
                  />
                  <p className="mt-5 text-white/40 text-xs leading-relaxed">
                    Open a supported provider URL from your own signed-in browser profile. Move
                    Party never receives your provider credentials.
                  </p>
                </div>
              </motion.div>
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

          {(status === "error" || error) && (
            <div
              className="mt-4 flex items-center gap-2 text-[#F87171] text-xs tracking-wider"
              data-testid="create-error"
            >
              <AlertCircle className="w-4 h-4" />{" "}
              {error ?? "Unsupported source. Try MP4, MKV or a direct URL."}
            </div>
          )}

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
              <div className="mt-4 flex items-center justify-between gap-4">
                <span className="text-[11px] tracking-[0.24em] uppercase text-white/40">
                  Your screening is ready to open
                </span>
                <div className="flex items-center gap-3">
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
                    {isCreating ? "Creating" : "Create cinema room"}
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
                Prepare Cinema
              </CinemaButton>
            </div>
          )}
        </motion.section>
      </main>
    </div>
  );
}
