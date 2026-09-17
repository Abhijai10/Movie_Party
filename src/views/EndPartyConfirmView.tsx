import { Button } from "../components/Button";

type EndPartyConfirmViewProps = {
  /**
   * F6: this confirmation is destructive and host-only. A guest pressing Leave
   * used to be told "the movie will stop for both participants" — wording that
   * does not apply to them, because the room is not theirs to end. The backend
   * already behaves correctly (`leave_party` only stops a host server, and a
   * guest has none), so this is the wording half of the fix.
   */
  isHost: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};

/** Title/body/action copy for the leave-or-end confirmation (F6). */
export function endPartyConfirmCopy(isHost: boolean): {
  title: string;
  body: string;
  confirmLabel: string;
} {
  if (isHost) {
    return {
      title: "End Movie Party for everyone?",
      body: "The movie will stop for both participants and the cached file choice will be shown next.",
      confirmLabel: "End Party",
    };
  }
  return {
    title: "Leave the movie party?",
    body: "You will leave the room. Your partner stays in the party, and you can rejoin with the invite.",
    confirmLabel: "Leave Party",
  };
}

export function EndPartyConfirmView({ isHost, onCancel, onConfirm }: EndPartyConfirmViewProps) {
  const copy = endPartyConfirmCopy(isHost);
  return (
    <main className="centered-shell">
      <section className="modal-panel" role="dialog" aria-modal="true" aria-labelledby="end-title">
        <h1 id="end-title">{copy.title}</h1>
        <div className="modal-body">
          <p>{copy.body}</p>
        </div>
        <div className="action-row">
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button variant={isHost ? "danger" : "primary"} onClick={onConfirm}>
            {copy.confirmLabel}
          </Button>
        </div>
      </section>
    </main>
  );
}
