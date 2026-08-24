type ReconnectOverlayProps = {
  peerName: string;
  isReconnecting: boolean;
};

export function ReconnectOverlay({ peerName, isReconnecting }: ReconnectOverlayProps) {
  if (!isReconnecting) {
    return null;
  }

  return (
    <section className="reconnect-overlay" aria-live="polite">
      <h2>{peerName} disconnected.</h2>
      <p>The movie has been paused. Reconnecting...</p>
    </section>
  );
}
