import { Component, type ErrorInfo, type ReactNode } from "react";

type ErrorBoundaryProps = {
  children: ReactNode;
};

type ErrorBoundaryState = {
  error: Error | null;
};

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = {
    error: null,
  };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("MP-UI-001 render failure", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <main className="centered-shell">
          <section className="modal-panel" role="alert" aria-live="assertive">
            <h1>Movie Party needs a quick refresh</h1>
            <div className="modal-body">
              <p>MP-UI-001 A rendering failure stopped this screen.</p>
            </div>
            <div className="action-row">
              <button
                type="button"
                className="mp-button primary"
                onClick={() => {
                  window.location.reload();
                }}
              >
                Reload Movie Party
              </button>
            </div>
          </section>
        </main>
      );
    }

    return this.props.children;
  }
}

export default ErrorBoundary;
