import {
  BackendCommandError,
  commandErrorMessage,
  createLocalParty,
  enterCinema,
  getProviderCapabilities,
  joinParty,
  launchGenericLink,
  launchProvider,
  leaveParty,
  markReady,
  requestEndParty,
  setSharedControls,
  showHome,
  showJoinParty,
  listenToDeepLinks,
  takePendingDeepLinks,
  type AppSnapshot,
  type ProviderCapability,
} from "../backend/appRuntime";
import { useAppSnapshot } from "../hooks/useAppSnapshot";
import { parseMovePartyInvite } from "../invites/deepLinks";
import { CinemaView } from "../views/CinemaView";
import { CreatePartyView, type CreatePartyRequest } from "../views/CreatePartyView";
import { EndPartyConfirmView } from "../views/EndPartyConfirmView";
import { HomeView } from "../views/HomeView";
import { JoinPartyView } from "../views/JoinPartyView";
import { LobbyView } from "../views/LobbyView";
import { ReadyCheckView } from "../views/ReadyCheckView";
import { LoadingState } from "./LoadingState";
import { useCallback, useEffect, useState } from "react";

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
  const [providerCapabilities, setProviderCapabilities] = useState<ProviderCapability[]>([]);
  const [pendingInvite, setPendingInvite] = useState("");
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

  const applySnapshot = useCallback((next: AppSnapshot | null) => {
    if (next) {
      setSnapshot(next);
    }
  }, [setSnapshot]);

  const openJoinWithInvite = useCallback(
    (rawInvite: string) => {
      const parsed = parseMovePartyInvite(rawInvite);
      setDevScreen(null);
      setLocalScreen(null);

      if (!parsed.ok) {
        setPendingInvite(rawInvite.trim());
        setJoinError(parsed.message);
        void showJoinParty().then(applySnapshot);
        return;
      }

      setPendingInvite(parsed.invite);
      setJoinError(null);
      void showJoinParty().then(applySnapshot);
    },
    [applySnapshot],
  );

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void takePendingDeepLinks().then((links) => {
      if (disposed) return;
      links.forEach(openJoinWithInvite);
    });

    void listenToDeepLinks((url) => {
      if (!disposed) {
        openJoinWithInvite(url);
      }
    }).then((nextUnlisten) => {
      if (disposed) {
        nextUnlisten();
        return;
      }
      unlisten = nextUnlisten;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [openJoinWithInvite]);

  useEffect(() => {
    void getProviderCapabilities().then(setProviderCapabilities);
  }, []);

  const goHome = () => {
    setDevScreen(null);
    setLocalScreen(null);
    setCreateError(null);
    setJoinError(null);
    setPendingInvite("");
    void showHome().then(applySnapshot);
  };

  const goCreateParty = () => {
    setDevScreen(null);
    setCreateError(null);
    setLocalScreen("CREATE_PARTY");
  };

  const createParty = async (request: CreatePartyRequest): Promise<boolean> => {
    const source = request.source === "local" ? request.mediaPath?.trim() ?? "" : request.url.trim();
    if (!source) {
      setCreateError("Select a movie file, provider page, or direct link.");
      return false;
    }

    setCreateError(null);
    setIsCreating(true);

    try {
      if (request.source === "provider") {
        if (request.mode !== "PROVIDER_SYNC") {
          setCreateError(
            "Provider Shared is experimental and unavailable until capture is verified on this device.",
          );
          return false;
        }
        const partySnapshot = await createLocalParty(null);
        if (partySnapshot?.screen !== "LOBBY") {
          applySnapshot(partySnapshot);
          setCreateError(partySnapshot?.error ?? "Move Party could not create the room.");
          return false;
        }

        const providerSnapshot = await launchProvider(request.providerId, source, request.mode);
        if (!providerSnapshot || providerSnapshot.error) {
          applySnapshot(partySnapshot);
          setCreateError(providerSnapshot?.error ?? "Move Party could not prepare that provider.");
          return false;
        }

        applySnapshot(providerSnapshot);
        setLocalScreen(null);
        return true;
      }

      if (request.source === "link") {
        const partySnapshot = await createLocalParty(null);
        if (partySnapshot?.screen !== "LOBBY") {
          applySnapshot(partySnapshot);
          setCreateError(partySnapshot?.error ?? "Move Party could not create the room.");
          return false;
        }

        const linkSnapshot = await launchGenericLink(source);
        if (!linkSnapshot || linkSnapshot.error) {
          applySnapshot(partySnapshot);
          setCreateError(linkSnapshot?.error ?? "Move Party could not prepare that link.");
          return false;
        }

        applySnapshot(linkSnapshot);
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
    setPendingInvite("");
    void showJoinParty().then(applySnapshot);
  };

  const submitJoin = async (inviteCode: string): Promise<boolean> => {
    const parsed = parseMovePartyInvite(inviteCode);
    if (!parsed.ok) {
      setJoinError(parsed.message);
      return false;
    }

    setJoinError(null);
    setIsJoining(true);
    try {
      const next = await joinParty(parsed.invite);
      if (next?.screen !== "LOBBY") {
        applySnapshot(next);
        setJoinError(next?.error ?? "Move Party could not join that invite.");
        return false;
      }

      applySnapshot(next);
      return true;
    } catch (error) {
      setJoinError(joinFailureMessage(error));
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
        providerCapabilities={providerCapabilities}
        onBack={goHome}
        onCreateParty={createParty}
      />
    );
  }

  if (snapshot.screen === "JOIN_PARTY" || devScreen === "JOIN") {
    return (
      <JoinPartyView
        isJoining={isJoining}
        error={joinError ?? snapshot.error}
        initialInvite={pendingInvite}
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
        onSnapshot={applySnapshot}
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

function joinFailureMessage(error: unknown): string {
  if (error instanceof BackendCommandError) {
    if (error.code === "MP-ROOM-001") {
      return "That invite is invalid, expired, or incomplete. Ask the host to copy a fresh invite.";
    }
    if (error.code.startsWith("MP-NET-")) {
      return "Move Party could not reach the host. Check that both devices are online and connected to Tailscale.";
    }
    return "Move Party could not join that room right now.";
  }

  return "Move Party could not join that invite.";
}
