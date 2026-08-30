import { Button } from "../components/Button";

type EndPartyConfirmViewProps = {
  onCancel: () => void;
  onConfirm: () => void;
};

export function EndPartyConfirmView({ onCancel, onConfirm }: EndPartyConfirmViewProps) {
  return (
    <main className="centered-shell">
      <section className="modal-panel" role="dialog" aria-modal="true" aria-labelledby="end-title">
        <h1 id="end-title">End Movie Party for everyone?</h1>
        <div className="modal-body">
          <p>
            The movie will stop for both participants and the cached file choice will be shown next.
          </p>
        </div>
        <div className="action-row">
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="danger" onClick={onConfirm}>
            End Party
          </Button>
        </div>
      </section>
    </main>
  );
}
