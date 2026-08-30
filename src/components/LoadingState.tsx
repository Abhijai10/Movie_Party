import logoMark from "../assets/logo_mark.png";

type LoadingStateProps = {
  title: string;
  message: string;
};

export function LoadingState({ title, message }: LoadingStateProps) {
  return (
    <main className="centered-shell">
      <section className="modal-panel" style={{ maxWidth: 420 }} aria-labelledby="loading-title">
        <img
          src={logoMark}
          alt="Movie Party logo"
          className="w-16 h-16 mx-auto mb-4 object-cover rounded-full"
          style={{ boxShadow: "0 0 24px rgba(159,122,234,0.4)" }}
        />
        <h1 id="loading-title">{title}</h1>
        <div className="modal-body">
          <p>{message}</p>
        </div>
      </section>
    </main>
  );
}
