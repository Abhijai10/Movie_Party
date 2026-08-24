type LoadingStateProps = {
  title: string;
  message: string;
};

export function LoadingState({ title, message }: LoadingStateProps) {
  return (
    <main className="centered-shell">
      <section className="modal-panel" style={{ maxWidth: 420 }} aria-labelledby="loading-title">
        <h1 id="loading-title">{title}</h1>
        <div className="modal-body">
          <p>{message}</p>
        </div>
      </section>
    </main>
  );
}
