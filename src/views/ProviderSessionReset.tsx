import { useState } from "react";
import { resetProviderSession, type AppSnapshot } from "../backend/appRuntime";
import { CinemaButton } from "../components/mp/CinemaButton";

export function ProviderSessionReset({
  providerId,
  name,
  onSnapshot,
}: {
  providerId: string;
  name: string;
  onSnapshot?: (snapshot: AppSnapshot) => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const reset = async () => {
    if (!confirming || busy) return;
    setBusy(true);
    setMessage(null);
    try {
      const snapshot = await resetProviderSession(providerId, true);
      onSnapshot?.(snapshot);
      setConfirming(false);
      setMessage(`Session reset. Open ${name} to sign in again.`);
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      setMessage(
        detail.includes("MP-PROVIDER-005")
          ? "The managed browser is still using this profile. Close its Movie Party window and retry."
          : "Could not reset the session. Leave the party and close its Movie Party browser, then retry. Your sign-in data has been kept where possible.",
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="mt-4 text-sm text-white/80">
      {confirming ? (
        <div role="group" aria-label={`Confirm reset for ${name}`} className="space-y-3">
          <p>
            Reset {name} on this device? This removes its saved sign-in and browser data. You will
            need to sign in again. Leave your party first.
          </p>
          <div className="flex flex-wrap gap-3">
            <CinemaButton
              variant="neutral"
              disabled={busy}
              onClick={() => {
                setConfirming(false);
                setMessage(null);
              }}
            >
              Cancel
            </CinemaButton>
            <CinemaButton
              variant="neutral"
              disabled={busy}
              onClick={() => {
                void reset();
              }}
            >
              {busy ? "Resetting session…" : "Confirm reset and sign out"}
            </CinemaButton>
          </div>
        </div>
      ) : (
        <CinemaButton
          variant="neutral"
          onClick={() => {
            setConfirming(true);
            setMessage(null);
          }}
        >
          Reset provider session…
        </CinemaButton>
      )}
      {message && (
        <p role="status" className="mt-3">
          {message}
        </p>
      )}
    </div>
  );
}
