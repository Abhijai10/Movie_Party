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
  leaveParty,
  markReady,
  listenToSnapshots,
  setSharedControls,
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
    void getAppSnapshot().then((next) => {
      applySnapshot(next ? { ...next, screen: "HOME" } : next);
    });
  };

  const goCreateParty = (mediaPath: string | null) => {
    void createLocalParty(mediaPath).then(applySnapshot);
  };

  const goJoinParty = () => {
    applySnapshot(snapshot ? { ...snapshot, screen: "JOIN_PARTY" } : snapshot);
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
    void leaveParty().then(applySnapshot);
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

  if (state === "PARTY_ENDED") {
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
            <button className="danger-action" type="button" onClick={goHome}>
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
