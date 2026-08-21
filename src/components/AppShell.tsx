import {
  createLocalParty,
  enterCinema,
  joinParty,
  launchProvider,
  leaveParty,
  markReady,
  requestEndParty,
  setSharedControls,
  showHome,
  showJoinParty,
  type AppSnapshot,
} from "../backend/appRuntime";
import { useAppSnapshot } from "../hooks/useAppSnapshot";
import { CinemaView } from "../views/CinemaView";
import { CreatePartyView } from "../views/CreatePartyView";
import { EndPartyConfirmView } from "../views/EndPartyConfirmView";
import { HomeView } from "../views/HomeView";
import { JoinPartyView } from "../views/JoinPartyView";
import { LobbyView } from "../views/LobbyView";
import { ReadyCheckView } from "../views/ReadyCheckView";
import { LoadingState } from "./LoadingState";
import { useState } from "react";

type LocalScreen = "CREATE_PARTY" | null;

export function AppShell() {
  const { snapshot, setSnapshot } = useAppSnapshot();
  const [localScreen, setLocalScreen] = useState<LocalScreen>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [isJoining, setIsJoining] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [joinError, setJoinError] = useState<string | null>(null);

  const applySnapshot = (next: AppSnapshot | null) => {
    if (next) {
      setSnapshot(next);
    }
  };

  const goHome = () => {
    setLocalScreen(null);
    setCreateError(null);
    setJoinError(null);
    void showHome().then(applySnapshot);
  };

  const goCreateParty = () => {
    setCreateError(null);
    setLocalScreen("CREATE_PARTY");
  };

  const createParty = async (mediaPath: string | null): Promise<boolean> => {
    const source = mediaPath?.trim() ?? "";
    if (!source) {
      setCreateError("Select a movie file or enter a provider URL.");
      return false;
    }

    const provider = providerIdForInput(source);
    setCreateError(null);
    setIsCreating(true);

    try {
      if (provider) {
        const partySnapshot = await createLocalParty(null);
        if (partySnapshot?.screen !== "LOBBY") {
          applySnapshot(partySnapshot);
          setCreateError(partySnapshot?.error ?? "Move Party could not create the room.");
          return false;
        }

        const providerSnapshot = await launchProvider(provider, source);
        if (!providerSnapshot) {
          applySnapshot(partySnapshot);
          setLocalScreen(null);
          return true;
        }

        applySnapshot(providerSnapshot);
        setLocalScreen(null);
        return true;
      }

      const next = await createLocalParty(source);
      if (next?.screen !== "LOBBY") {
        applySnapshot(next);
        setCreateError(next?.error ?? "Move Party could not create the room.");
        return false;
      }

      applySnapshot(next);
      setLocalScreen(null);
      return true;
    } finally {
      setIsCreating(false);
    }
  };

  const goJoinParty = () => {
    setLocalScreen(null);
    setJoinError(null);
    void showJoinParty().then(applySnapshot);
  };

  const submitJoin = async (inviteCode: string): Promise<boolean> => {
    const code = inviteCode.trim();
    if (!code) {
      setJoinError("Enter an invite link or code.");
      return false;
    }

    setJoinError(null);
    setIsJoining(true);
    try {
      const next = await joinParty(code);
      if (next?.screen !== "LOBBY") {
        applySnapshot(next);
        setJoinError(next?.error ?? "Move Party could not join that invite.");
        return false;
      }

      applySnapshot(next);
      return true;
    } finally {
      setIsJoining(false);
    }
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

  const requestPartyEnd = () => {
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
      <LoadingState
        title="Starting Move Party"
        message="Connecting to the local app runtime."
      />
    );
  }

  if (localScreen === "CREATE_PARTY") {
    return (
      <CreatePartyView
        snapshot={snapshot}
        isCreating={isCreating}
        error={createError ?? snapshot.error}
        onBack={goHome}
        onCreateLocalParty={createParty}
      />
    );
  }

  if (snapshot.screen === "JOIN_PARTY") {
    return (
      <JoinPartyView
        snapshot={snapshot}
        isJoining={isJoining}
        error={joinError ?? snapshot.error}
        onBack={goHome}
        onJoin={submitJoin}
      />
    );
  }

  if (snapshot.screen === "LOBBY") {
    return (
      <LobbyView
        snapshot={snapshot}
        onBack={goHome}
        onReady={goReadyCheck}
        onCinema={goCinema}
        onToggleSharedControls={handleToggleSharedControls}
      />
    );
  }

  if (snapshot.screen === "READY_CHECK") {
    return <ReadyCheckView snapshot={snapshot} onStart={goCinema} />;
  }

  if (snapshot.screen === "CINEMA") {
    return <CinemaView snapshot={snapshot} onSnapshot={setSnapshot} onLeave={requestPartyEnd} />;
  }

  if (snapshot.screen === "PARTY_END_CONFIRM") {
    return (
      <EndPartyConfirmView
        onCancel={goCinema}
        onConfirm={confirmEndParty}
      />
    );
  }

  return <HomeView snapshot={snapshot} onCreate={goCreateParty} onJoin={goJoinParty} />;
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
