type BufferingOverlayProps = {
  bufferingParticipant: string | null;
  peerName: string;
  percent: number;
  strictSyncPaused: boolean;
};

export function BufferingOverlay({
  bufferingParticipant,
  peerName,
  percent,
  strictSyncPaused,
}: BufferingOverlayProps) {
  if (!strictSyncPaused && !bufferingParticipant) {
    return null;
  }

  return (
    <section className="buffer-overlay" aria-live="polite">
      <h1>Paused to keep you together</h1>
      <p>{bufferingParticipant ?? peerName} is buffering</p>
      <div className="meter">
        <span style={{ width: `${String(percent)}%` }} />
      </div>
      <small>Camera and chat are still available.</small>
    </section>
  );
}
