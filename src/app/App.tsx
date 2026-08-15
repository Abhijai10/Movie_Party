import { CinemaMode } from "../cinema/CinemaMode";
import { HomeScreen } from "../lobby/HomeScreen";
import {
  CreatePartyScreen,
  JoinPartyScreen,
  LobbyScreen,
  ReadyCheckScreen,
} from "../party/PartyScreens";
import type { AppState } from "../types/app-state";
import { useState } from "react";

export function App() {
  const [state, setState] = useState<AppState>("HOME");
  const goHome = () => {
    setState("HOME");
  };
  const goCreateParty = () => {
    setState("CREATE_PARTY");
  };
  const goJoinParty = () => {
    setState("JOIN_PARTY");
  };
  const goLobby = () => {
    setState("LOBBY");
  };
  const goReadyCheck = () => {
    setState("READY_CHECK");
  };
  const goCinema = () => {
    setState("CINEMA");
  };
  const goPartyEnded = () => {
    setState("PARTY_ENDED");
  };

  if (state === "CREATE_PARTY") {
    return <CreatePartyScreen onBack={goHome} onStart={goLobby} />;
  }

  if (state === "JOIN_PARTY") {
    return <JoinPartyScreen onBack={goHome} onJoin={goLobby} />;
  }

  if (state === "LOBBY") {
    return <LobbyScreen onBack={goHome} onReady={goReadyCheck} onCinema={goCinema} />;
  }

  if (state === "READY_CHECK") {
    return <ReadyCheckScreen onStart={goCinema} />;
  }

  if (state === "CINEMA") {
    return <CinemaMode onLeave={goPartyEnded} />;
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
      <HomeScreen onContinue={goCreateParty} onChooseMovie={goCreateParty} onJoin={goJoinParty} />
    </main>
  );
}
