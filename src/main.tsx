import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./styles.css";

const rootElement = document.getElementById("root");

if (!rootElement) {
  throw new Error("Movie Party root element was not found.");
}

createRoot(rootElement).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);

// The boot splash (index.html) painted instantly while the bundle loaded;
// this file running means React is mounted. Fade the splash away and let
// the app's own loading state take over. The safety timeout guarantees the
// splash can never stick around even if the fade transition never fires.
const bootSplash = document.getElementById("boot-splash");
if (bootSplash) {
  window.setTimeout(() => {
    bootSplash.style.transition = "opacity 220ms ease-out";
    bootSplash.style.opacity = "0";
    bootSplash.addEventListener(
      "transitionend",
      () => {
        bootSplash.remove();
      },
      { once: true },
    );
  }, 60);
  window.setTimeout(() => {
    bootSplash.remove();
  }, 4000);
}
