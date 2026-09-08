import { useState } from "react";
import { FolderOpen, Trash2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import {
  retentionKeep,
  retentionRemove,
  retentionSaveAs,
} from "../../backend/appRuntime";

/**
 * §52 LOCAL MEDIA RETENTION PROMPT — asked once when a Local Perfect party
 * ends on the device that received the movie. The question is explicit:
 * nothing is deleted or kept silently. Default focus is Remove (the
 * conservative default); no automatic deletion happens while the app
 * remains open without an answer.
 *
 * §52 option order: Remove / Keep in Movie Party / Save As…
 */
export function RetentionPrompt({
  mediaId,
  filename,
  onDecided,
}: {
  mediaId: string;
  filename: string;
  onDecided: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const run = async (action: () => Promise<boolean>) => {
    if (busy) return;
    setBusy(true);
    const ok = await action();
    setBusy(false);
    if (ok) {
      onDecided();
    } else {
      setNotice("The decision could not be applied — try again.");
    }
  };

  const saveAs = async () => {
    if (busy) return;
    setBusy(true);
    let folder: string | null = null;
    try {
      folder = await invoke<string>("pick_save_folder");
    } catch {
      // Cancelled folder pick — not an error, just no decision yet.
      setBusy(false);
      return;
    }
    const saved = await retentionSaveAs(mediaId, folder);
    setBusy(false);
    if (saved != null) {
      onDecided();
    } else {
      setNotice("MP-MEDIA-002 the export failed — choose the folder again.");
    }
  };

  return (
    <div
      className="fixed inset-0 z-[70] flex items-center justify-center p-6"
      style={{ background: "rgba(5,4,8,0.78)", backdropFilter: "blur(8px)" }}
      role="dialog"
      aria-modal="true"
      aria-label="Keep this movie on this device?"
      data-testid="retention-prompt"
    >
      <div className="w-full max-w-md rounded-2xl bg-[#0D0B14] border border-white/12 p-6">
        <span className="text-[11px] tracking-[0.28em] uppercase text-white/45">
          Movie Party · after the party
        </span>
        <h2 className="mt-3 font-serif-display text-2xl text-white">
          Keep <span className="italic">{filename}</span> on this device?
        </h2>
        <p className="mt-2 text-sm text-white/55 leading-relaxed">
          The movie was transferred to this device for the party. It stays in
          Movie Party's cache until you decide.
        </p>

        {notice != null ? (
          <p
            className="mt-3 text-xs text-[#F87171]"
            role="alert"
            data-testid="retention-notice"
          >
            {notice}
          </p>
        ) : null}

        <div className="mt-5 grid gap-2.5">
          <button
            type="button"
            autoFocus
            disabled={busy}
            onClick={() => {
              void run(() => retentionRemove(mediaId));
            }}
            className="flex items-center justify-between rounded-xl border border-white/10 bg-white/[0.03] hover:bg-white/[0.06] px-4 py-3 text-left text-sm text-white/85 transition disabled:opacity-50"
            data-testid="retention-remove-btn"
          >
            <span>Remove from this device</span>
            <Trash2 className="w-4 h-4 text-white/40" />
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              void run(() => retentionKeep(mediaId));
            }}
            className="flex items-center justify-between rounded-xl border border-white/10 bg-white/[0.03] hover:bg-white/[0.06] px-4 py-3 text-left text-sm text-white/85 transition disabled:opacity-50"
            data-testid="retention-keep-btn"
          >
            <span>Keep in Movie Party</span>
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              void saveAs();
            }}
            className="flex items-center justify-between rounded-xl border border-white/10 bg-white/[0.03] hover:bg-white/[0.06] px-4 py-3 text-left text-sm text-white/85 transition disabled:opacity-50"
            data-testid="retention-save-as-btn"
          >
            <span>Save As… (choose a folder)</span>
            <FolderOpen className="w-4 h-4 text-white/40" />
          </button>
        </div>
        <p className="mt-4 text-[10px] text-white/35 leading-relaxed">
          Only Movie Party's own cache is touched — the host's original file is
          never modified.
        </p>
      </div>
    </div>
  );
}
