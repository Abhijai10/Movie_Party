import {
  commandErrorMessage,
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
import { useEffect, useState } from "react";

type LocalScreen = "CREATE_PARTY" | null;
type DevScreen = "HOME" | "CREATE" | "JOIN" | "LOBBY" | "READY" | "CINEMA" | null;
const developmentPreviewEnabled =
  typeof window !== "undefined" &&
  (window.location.hostname === "localhost" || window.location.hostname === "127.0.0.1");

export function AppShell() {
  const { snapshot, setSnapshot } = useAppSnapshot();
  const [localScreen, setLocalScreen] = useState<LocalScreen>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [isJoining, setIsJoining] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [joinError, setJoinError] = useState<string | null>(null);
  const [devScreen, setDevScreen] = useState<DevScreen>(null);

  useEffect(() => {
    if (!developmentPreviewEnabled) return;
    const handlePreviewKey = (event: KeyboardEvent) => {
      if (!event.altKey || event.key < "1" || event.key > "6") return;
      const screens: Exclude<DevScreen, null>[] = [
        "HOME",
        "CREATE",
        "JOIN",
        "LOBBY",
        "READY",
        "CINEMA",
      ];
      setDevScreen(screens[Number(event.key) - 1] ?? null);
    };
    window.addEventListener("keydown", handlePreviewKey);
    return () => {
      window.removeEventListener("keydown", handlePreviewKey);
    };
  }, []);

  const applySnapshot = (next: AppSnapshot | null) => {
    if (next) {
      setSnapshot(next);
    }
  };

  const goHome = () => {
    setDevScreen(null);
    setLocalScreen(null);
    setCreateError(null);
    setJoinError(null);
    void showHome().then(applySnapshot);
  };

  const goCreateParty = () => {
    setDevScreen(null);
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
    } catch (error) {
      setCreateError(createRoomErrorMessage(error));
      return false;
    } finally {
      setIsCreating(false);
    }
  };

  const goJoinParty = () => {
    setDevScreen(null);
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
    } catch (error) {
      setJoinError(commandErrorMessage(error, "Move Party could not join that invite."));
      return false;
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
      <LoadingState title="Starting Move Party" message="Connecting to the local app runtime." />
    );
  }

  if (devScreen === "HOME") return <HomeView onCreate={goCreateParty} onJoin={goJoinParty} />;

  if (localScreen === "CREATE_PARTY" || devScreen === "CREATE") {
    return (
      <CreatePartyView
        isCreating={isCreating}
        error={createError ?? snapshot.error}
        onBack={goHome}
        onCreateLocalParty={createParty}
      />
    );
  }

  if (snapshot.screen === "JOIN_PARTY" || devScreen === "JOIN") {
    return (
      <JoinPartyView
        isJoining={isJoining}
        error={joinError ?? snapshot.error}
        onBack={goHome}
        onJoin={submitJoin}
      />
    );
  }

  if (snapshot.screen === "LOBBY" || devScreen === "LOBBY") {
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

  if (snapshot.screen === "READY_CHECK" || devScreen === "READY") {
    return <ReadyCheckView snapshot={snapshot} onStart={goCinema} />;
  }

  if (snapshot.screen === "CINEMA" || devScreen === "CINEMA") {
    return <CinemaView snapshot={snapshot} onSnapshot={setSnapshot} onLeave={requestPartyEnd} />;
  }

  if (snapshot.screen === "PARTY_END_CONFIRM") {
    return <EndPartyConfirmView onCancel={goCinema} onConfirm={confirmEndParty} />;
  }

  return <HomeView onCreate={goCreateParty} onJoin={goJoinParty} />;
}

function createRoomErrorMessage(error: unknown): string {
  const fallback = "Move Party could not create the room.";
  if (!developmentPreviewEnabled) {
    return fallback;
  }

  return commandErrorMessage(error, fallback);
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
