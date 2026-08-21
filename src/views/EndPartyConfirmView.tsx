import { Button } from "../components/Button";
import { Modal } from "../components/Modal";

type EndPartyConfirmViewProps = {
  onCancel: () => void;
  onConfirm: () => void;
};

export function EndPartyConfirmView({ onCancel, onConfirm }: EndPartyConfirmViewProps) {
  return (
    <Modal
      title="End Move Party for everyone?"
      actions={
        <>
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="danger" onClick={onConfirm}>
            End Party
          </Button>
        </>
      }
    >
      <p>The movie will stop for both participants and the cached file choice will be shown next.</p>
    </Modal>
  );
}
