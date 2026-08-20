import { CinemaMode } from "../cinema/CinemaMode";
import { HomeScreen } from "../lobby/HomeScreen";
import {
  JoinPartyScreen,
  LobbyScreen,
  ReadyCheckScreen,
} from "../party/PartyScreens";
import {
  createLocalParty,
  enterCinema,
  getAppSnapshot,
  joinParty,
  launchProvider,
  leaveParty,
  markReady,
  listenToSnapshots,
  requestEndParty,
  setSharedControls,
  showHome,
  showJoinParty,
  type AppSnapshot,
} from "../backend/appRuntime";
import { type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

export function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const state = snapshot?.screen ?? "BOOTING";

  useEffect(() => {
    let isMounted = true;
    let unlisten: UnlistenFn | null = null;

    // Register listener before fetching the initial snapshot to avoid race conditions
    void listenToSnapshots((next) => {
      if (isMounted) {
        setSnapshot(next);
      }
    }).then((unlistenFn) => {
      if (isMounted) {
        unlisten = unlistenFn;
      } else {
        unlistenFn();
      }
    });

    void getAppSnapshot().then((next) => {
      if (isMounted) {
        setSnapshot(next);
      }
    });

    return () => {
      isMounted = false;
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  const applySnapshot = (next: AppSnapshot | null) => {
    if (next) {
      setSnapshot(next);
    }
  };

  const goHome = () => {
    void showHome().then(applySnapshot);
  };

  const goCreateParty = (mediaPath: string | null) => {
    const provider = providerIdForInput(mediaPath);
    if (provider && mediaPath) {
      void createLocalParty(null).then((partySnapshot) => {
        applySnapshot(partySnapshot);
        void launchProvider(provider, mediaPath).then(applySnapshot);
      });
      return;
    }

    void createLocalParty(mediaPath).then(applySnapshot);
  };

  const goJoinParty = () => {
    void showJoinParty().then(applySnapshot);
  };

  const submitJoin = (inviteCode: string) => {
    void joinParty(inviteCode).then(applySnapshot);
  };

  const goReadyCheck = () => {
    void markReady().then(applySnapshot);
  };

  const goCinema = () => {
    void enterCinema().then(applySnapshot);
  };

  const handleToggleSharedControls = (enabled: boolean) => {
    void setSharedControls(enabled).then(applySnapshot);
  };

  const goPartyEnded = () => {
    void requestEndParty().then(applySnapshot);
  };

  const confirmEndParty = () => {
    void leaveParty().then((ended) => {
      applySnapshot(ended);
      void showHome().then(applySnapshot);
    });
  };

  if (!snapshot) {
    return (
      <main className="app-shell centered-shell">
        <section className="setup-panel" aria-labelledby="boot-title">
          <h1 id="boot-title">Starting Move Party</h1>
          <p className="panel-copy">Connecting to the local app runtime.</p>
        </section>
      </main>
    );
  }

  if (state === "JOIN_PARTY") {
    return <JoinPartyScreen snapshot={snapshot} onBack={goHome} onJoin={submitJoin} />;
  }

  if (state === "LOBBY") {
    return (
      <LobbyScreen
        snapshot={snapshot}
        onBack={goHome}
        onReady={goReadyCheck}
        onCinema={goCinema}
        onToggleSharedControls={handleToggleSharedControls}
      />
    );
  }

  if (state === "READY_CHECK") {
    return <ReadyCheckScreen snapshot={snapshot} onStart={goCinema} />;
  }

  if (state === "CINEMA") {
    return <CinemaMode snapshot={snapshot} onSnapshot={setSnapshot} onLeave={goPartyEnded} />;
  }

  if (state === "PARTY_END_CONFIRM") {
    return (
      <main className="app-shell centered-shell">
        <section
          className="dialog-panel"
          aria-labelledby="ended-title"
          role="dialog"
          aria-modal="true"
        >
          <h1 id="ended-title">End Move Party for everyone?</h1>
          <p>
            The movie will stop for both participants and the cached file choice will be shown next.
          </p>
          <div className="action-row">
            <button className="secondary-action" type="button" onClick={goCinema}>
              Cancel
            </button>
            <button className="danger-action" type="button" onClick={confirmEndParty}>
              End Party
            </button>
          </div>
        </section>
      </main>
    );
  }

  return (
    <main className="app-shell">
      <HomeScreen snapshot={snapshot} onCreateLocalParty={goCreateParty} onJoin={goJoinParty} />
    </main>
  );
}

function providerIdForInput(input: string | null): string | null {
  const value = input?.trim();
  if (!value) {
    return null;
  }

  let host = "";
  try {
    host = new URL(value).hostname.toLowerCase();
  } catch {
    return null;
  }

  if (host === "youtu.be" || host === "youtube.com" || host.endsWith(".youtube.com")) {
    return "youtube";
  }
  if (host === "netflix.com" || host.endsWith(".netflix.com")) {
    return "netflix";
  }
  if (
    host === "primevideo.com" ||
    host.endsWith(".primevideo.com") ||
    host === "amazon.com" ||
    host.endsWith(".amazon.com")
  ) {
    return "prime";
  }
  if (
    host === "hotstar.com" ||
    host.endsWith(".hotstar.com") ||
    host === "jiocinema.com" ||
    host.endsWith(".jiocinema.com")
  ) {
    return "jiohotstar";
  }

  return null;
}
