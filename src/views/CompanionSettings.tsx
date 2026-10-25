import { useCallback, useEffect, useState } from "react";
import {
  getCompanionStatus,
  installCompanionHost,
  newCompanionCode,
  removeCompanionHost,
  startCompanion,
  stopCompanion,
  unlinkCompanion,
  type CompanionStatus,
} from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";
import {
  companionCapabilityLines,
  companionHeadline,
  companionSubline,
  usablePairingCode,
} from "../providers/companionState";

/**
 * Settings › Companion. Every action here is a deliberate click: Movie Party
 * never installs, loads or talks to a browser extension by itself, and managed
 * Chrome stays the default provider path whether or not this is switched on.
 */
export function CompanionSettings() {
  const [status, setStatus] = useState<CompanionStatus | null>(null);
  const [extensionId, setExtensionId] = useState("");
  const [installedPath, setInstalledPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const run = useCallback(async (action: () => Promise<CompanionStatus | string | undefined>) => {
    setBusy(true);
    setNotice(null);
    try {
      const result = await action();
      if (typeof result === "string") setInstalledPath(result);
      else if (result) setStatus(result);
      else setStatus(await getCompanionStatus());
    } catch (reason: unknown) {
      setNotice(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    let active = true;
    void getCompanionStatus()
      .then((value) => {
        if (active) setStatus(value);
      })
      .catch(() => {
        if (active) setNotice("The companion status could not be read.");
      });
    return () => {
      active = false;
    };
  }, []);

  const code = status ? usablePairingCode(status) : null;

  return (
    <section className="companion-settings" aria-label="Existing browser companion">
      <p role="status">{companionHeadline(status)}</p>
      <p>{companionSubline(status)}</p>

      <div className="companion-actions">
        {status?.listening ? (
          <CinemaButton
            variant="neutral"
            disabled={busy}
            onClick={() => void run(() => stopCompanion())}
          >
            Turn companion off
          </CinemaButton>
        ) : (
          <CinemaButton disabled={busy || !status} onClick={() => void run(() => startCompanion())}>
            Use my existing browser
          </CinemaButton>
        )}
        {status?.listening && (
          <CinemaButton
            variant="neutral"
            disabled={busy}
            onClick={() => void run(() => newCompanionCode())}
          >
            New pairing code
          </CinemaButton>
        )}
        {status?.paired && (
          <CinemaButton
            variant="neutral"
            disabled={busy}
            onClick={() => void run(() => unlinkCompanion())}
          >
            Unlink browser
          </CinemaButton>
        )}
      </div>

      {code && (
        <p className="companion-code">
          Pairing code: <strong>{code}</strong>
        </p>
      )}

      {status?.listening && (
        <div className="companion-install">
          <label htmlFor="companion-extension-id">Extension ID from your browser</label>
          <input
            id="companion-extension-id"
            value={extensionId}
            onChange={(event) => {
              setExtensionId(event.target.value);
            }}
            placeholder="32 lowercase letters"
            maxLength={80}
            spellCheck={false}
            disabled={busy}
          />
          <div className="companion-actions">
            <CinemaButton
              variant="neutral"
              disabled={busy || extensionId.trim().length !== 32}
              onClick={() => void run(() => installCompanionHost(extensionId.trim()))}
            >
              Install browser host
            </CinemaButton>
            <CinemaButton
              variant="neutral"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await removeCompanionHost();
                  return undefined;
                })
              }
            >
              Remove browser host
            </CinemaButton>
          </div>
          {installedPath && <p role="status">Host manifest written to {installedPath}</p>}
        </div>
      )}

      {status && (
        <ul aria-label="What the companion may do">
          {companionCapabilityLines(status).map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      )}

      {status?.error && <p role="alert">{status.guidance ?? status.error}</p>}
      {notice && <p role="alert">{notice}</p>}
      <p className="companion-note">
        The companion has not been verified against a real provider page or a second device. If it
        cannot do something, Movie Party says so instead of pretending.
      </p>
    </section>
  );
}
