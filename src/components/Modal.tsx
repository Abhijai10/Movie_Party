import type { ReactNode } from "react";

type ModalProps = {
  title: string;
  children: ReactNode;
  actions: ReactNode;
};

export function Modal({ title, children, actions }: ModalProps) {
  return (
    <main className="app-shell centered-shell">
      <section className="modal-panel" aria-labelledby="modal-title" role="dialog" aria-modal="true">
        <h1 id="modal-title">{title}</h1>
        <div className="modal-body">{children}</div>
        <div className="action-row">{actions}</div>
      </section>
    </main>
  );
}
