import { useEffect, useRef, useState } from "react";
import { CheckCircle2, ExternalLink, Loader2, RefreshCw } from "lucide-react";
import type { ProviderCapability } from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
import {
  browseProviderContent,
  canCreateSelectedContent,
  confirmProviderContent,
  createContentQueue,
  getProviderContent,
  type ContentChooserSnapshot,
} from "../providers/contentChooser";
import {
  providerEvidence,
  providerEvidenceLabel,
  providerEvidenceTierLabel,
} from "../providers/roomProviderState";
import type { CreatePartyRequest } from "./CreatePartyView";

export function ProviderContentChooser({
  capabilities,
  providerId,
  onProviderChange,
  onOpenProvider,
  onCreateParty,
  isCreating,
  inRoom = false,
}: {
  capabilities: ProviderCapability[];
  providerId: string;
  onProviderChange: (id: string) => void;
  onOpenProvider: (id: string) => Promise<boolean>;
  onCreateParty?: (request: CreatePartyRequest) => Promise<boolean>;
  inRoom?: boolean;
  isCreating: boolean;
}) {
  const [snapshot, setSnapshot] = useState<ContentChooserSnapshot | null>(null);
  const [previewId, setPreviewId] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [direct, setDirect] = useState(false);
  const [busy, setBusy] = useState(false);
  const [fresh, setFresh] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [pollMessage, setPollMessage] = useState<string | null>(null);
  const [epoch, setEpoch] = useState(0);
  const queue = useRef(createContentQueue());
  const actionActive = useRef(false);
  const creating = useRef(isCreating);
  creating.current = isCreating;
  const current = useRef(providerId);
  current.current = providerId;
  const provider = capabilities.find((p) => p.id === providerId);
  const preview = snapshot?.tabs.find((t) => t.targetId === previewId) ?? null;
  const canCreate =
    preview?.identity?.providerId === providerId &&
    canCreateSelectedContent(snapshot, preview, fresh);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    setSnapshot(null);
    setPreviewId(null);
    setFresh(false);
    setMessage(null);
    const poll = async () => {
      try {
        if (actionActive.current || creating.current) return;
        const next = await queue.current(() =>
          disposed || actionActive.current || creating.current
            ? Promise.resolve(null)
            : getProviderContent(providerId),
        );
        if (disposed || !next) return;
        setSnapshot(next);
        setFresh(true);
        setPollMessage(null);
      } catch (error) {
        if (disposed) return;
        setFresh(false);
        setPollMessage(String(error));
      } finally {
        if (!disposed)
          timer = setTimeout(() => {
            void poll();
          }, 2500);
      }
    };
    if (providerId) void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [providerId, epoch]);

  const run = async (work: () => Promise<ContentChooserSnapshot | null>) => {
    if (actionActive.current || isCreating) return;
    actionActive.current = true;
    const id = providerId;
    setBusy(true);
    setMessage(null);
    setFresh(false);
    try {
      const next = await queue.current(work);
      if (current.current !== id) return;
      if (next) {
        setSnapshot(next);
        setFresh(true);
      }
    } catch (error) {
      if (current.current === id) setMessage(String(error));
    } finally {
      actionActive.current = false;
      setBusy(false);
    }
  };

  return (
    <section className="provider-chooser" aria-label="Choose provider content">
      <h2>Choose what to watch</h2>
      <ol className="provider-chooser-steps" aria-label="Selection steps">
        <li>Provider</li>
        <li>Browse or search</li>
        <li>Preview</li>
        <li>Confirm</li>
      </ol>
      <label htmlFor="provider-source-select">Choose provider</label>
      <select
        id="provider-source-select"
        className="provider-select"
        value={providerId}
        disabled={inRoom || busy || isCreating || !capabilities.length}
        data-testid="provider-select"
        onChange={(event) => {
          onProviderChange(event.target.value);
        }}
      >
        {!capabilities.length && <option value="">Checking providers…</option>}
        {capabilities.map((p) => (
          <option key={p.id} value={p.id}>
            {p.displayName}
          </option>
        ))}
      </select>
      <p>
        Provider Sync plays independently on both devices. Each viewer uses their own account.
        Provider Shared is off.
      </p>
      {provider && (
        <details className="provider-chooser-evidence">
          <summary>What is actually verified</summary>
          <p role="note">
            {providerEvidenceLabel(providerEvidence(provider.actions))} Check each provider before
            relying on it: docs/PROVIDER_VERIFICATION_CHECKLIST.md
          </p>
          <ul>
            {provider.actions.map((action) => (
              <li key={action.operation}>
                <span>{action.operation.replaceAll("_", " ")}</span>
                <em className={`provider-evidence-${action.evidence.toLowerCase()}`}>
                  {providerEvidenceTierLabel(action.evidence)}
                </em>
                <p>{action.note}</p>
              </li>
            ))}
          </ul>
        </details>
      )}
      <div className="provider-chooser-actions">
        <CinemaButton
          variant="neutral"
          disabled={busy || isCreating || !provider}
          icon={ExternalLink}
          onClick={() => {
            void run(async () => {
              const opened = await onOpenProvider(providerId);
              if (!opened)
                throw new Error(
                  "Could not open the managed browser. Check the message above, then retry.",
                );
              return getProviderContent(providerId);
            });
          }}
        >
          {snapshot?.browserReady && fresh
            ? "Reopen or check managed Chrome"
            : `Open ${provider?.displayName ?? "provider"} in Chrome`}
        </CinemaButton>
        <CinemaButton
          variant="ghost"
          disabled={busy || isCreating}
          icon={RefreshCw}
          onClick={() => {
            void run(() => getProviderContent(providerId));
          }}
        >
          Refresh tabs
        </CinemaButton>
      </div>
      <p role="status">
        {snapshot?.browserReady && fresh ? "Browser ready" : "Open the managed browser to begin"} ·{" "}
        {canCreate ? "Selected video ready" : "Content not ready for a party"}
      </p>

      {providerId === "youtube" && (
        <p>
          No YouTube search API is configured. Search opens YouTube in your managed browser, or
          paste a video URL.
        </p>
      )}
      <div role="group" aria-label="Find content" className="provider-chooser-find">
        <div role="group" aria-label="Content lookup mode" className="provider-chooser-actions">
          <CinemaButton
            variant={direct ? "ghost" : "primary"}
            aria-pressed={!direct}
            onClick={() => {
              setDirect(false);
            }}
          >
            Search by title
          </CinemaButton>
          <CinemaButton
            variant={direct ? "primary" : "ghost"}
            aria-pressed={direct}
            onClick={() => {
              setDirect(true);
            }}
          >
            Paste video or title URL
          </CinemaButton>
        </div>
        <label htmlFor="provider-content-input">
          {direct ? "Provider URL" : `Search ${provider?.displayName ?? "provider"}`}
        </label>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void run(() => browseProviderContent(providerId, input, direct));
          }}
        >
          <input
            id="provider-content-input"
            type={direct ? "url" : "search"}
            maxLength={direct ? 4096 : 300}
            value={input}
            onChange={(event) => {
              setInput(event.target.value);
            }}
            placeholder={direct ? "https://…" : "Movie, show or video title"}
            required
            disabled={busy || isCreating}
          />
          <CinemaButton
            disabled={busy || isCreating || !snapshot?.browserReady || !input.trim()}
            onClick={() => {
              void run(() => browseProviderContent(providerId, input, direct));
            }}
          >
            {direct ? "Open URL in Chrome" : "Search in Chrome"}
          </CinemaButton>
        </form>
        <p>
          Choose a result in Chrome, then preview its tab here. Sign in only on the provider’s own
          page.
          {providerId === "youtube" &&
            " Playlist links open the playlist; choose one video before confirming."}
        </p>
      </div>

      {(message || pollMessage || snapshot?.issue) && (
        <div role="alert" className="provider-chooser-notice">
          <p>{message ?? pollMessage ?? snapshot?.issue}</p>
          <CinemaButton
            variant="neutral"
            disabled={busy || isCreating}
            onClick={() => {
              setEpoch((value) => value + 1);
            }}
          >
            Retry and refresh
          </CinemaButton>
          <p>
            For a closed browser, use Open above. For a closed or changed tab, choose and confirm it
            again.
          </p>
        </div>
      )}
      {busy && (
        <p role="status" className="provider-chooser-actions">
          <Loader2 size={16} className="animate-spin" /> Checking the managed browser…
        </p>
      )}

      <h3>Select content from your provider tabs</h3>
      {!snapshot?.tabs.length && (
        <p>
          Open a title in managed Chrome. Its real title will appear here; home and search pages
          cannot be confirmed.
        </p>
      )}
      <ul className="provider-chooser-tabs">
        {snapshot?.tabs.map((tab) => (
          <li key={tab.targetId}>
            <div>
              <strong>{tab.title || "Waiting for title"}</strong>
              <p>
                {tab.issue ??
                  (tab.targetId === snapshot.suggestedTargetId
                    ? "Active provider tab"
                    : "Provider tab")}
              </p>
            </div>
            <CinemaButton
              variant={previewId === tab.targetId ? "primary" : "ghost"}
              disabled={busy || isCreating || !tab.identity || Boolean(tab.issue)}
              aria-pressed={previewId === tab.targetId}
              onClick={() => {
                setPreviewId(tab.targetId);
              }}
            >
              Preview title
            </CinemaButton>
          </li>
        ))}
      </ul>
      {preview?.identity && (
        <section className="provider-content-preview" aria-label="Title preview">
          {preview.thumbnailUrl && (
            <img
              src={preview.thumbnailUrl}
              alt={`Thumbnail for ${preview.title}`}
              referrerPolicy="no-referrer"
            />
          )}
          <div>
            <p>{provider?.displayName}</p>
            <h3>{preview.title}</h3>
            <p className="provider-content-url">{preview.identity.allowedUrl}</p>
            <p>
              {preview.playbackReady
                ? "Video loaded in Chrome"
                : "Open playback in Chrome and wait for the video to load."}
            </p>
            {snapshot?.confirmedKey === preview.selectionKey ? (
              <p className="provider-chooser-actions">
                <CheckCircle2 size={16} /> Selection confirmed
              </p>
            ) : (
              <CinemaButton
                disabled={busy || isCreating || !fresh || Boolean(preview.issue)}
                onClick={() => {
                  void run(() =>
                    confirmProviderContent(providerId, preview.targetId, preview.selectionKey),
                  );
                }}
              >
                Confirm selection
              </CinemaButton>
            )}
          </div>
        </section>
      )}
      {!inRoom && (
        <CinemaButton
          disabled={busy || isCreating || !canCreate}
          data-testid="create-room-btn"
          onClick={() => {
            if (!canCreate || !preview.identity || !snapshot?.confirmedKey) return;
            const url = preview.identity.allowedUrl;
            const selectionKey = snapshot.confirmedKey;
            void run(async () => {
              await onCreateParty?.({
                source: "provider",
                providerId,
                url,
                mode: "PROVIDER_SYNC",
                selectionKey,
              });
              return null;
            });
          }}
        >
          {isCreating ? "Creating party…" : "Create party"}
        </CinemaButton>
      )}
    </section>
  );
}
