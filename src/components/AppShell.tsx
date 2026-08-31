import {
  BackendCommandError,
  checkProviderStatus,
  commandErrorMessage,
  createLocalParty,
  enterCinema,
  getProviderCapabilities,
  getTailscaleReadiness,
  joinParty,
  launchGenericLink,
  launchProvider,
  leaveParty,
  markReady,
  navigateProviderTitle,
  openProviderBrowser,
  openTailscaleSetup,
  requestEndParty,
  setSharedControls,
  showHome,
  showJoinParty,
  listenToDeepLinks,
  takePendingDeepLinks,
  type AppSnapshot,
  type ProviderCapability,
  type TailscaleReadiness,
} from "../backend/appRuntime";
import { useAppSnapshot } from "../hooks/useAppSnapshot";
import { parseMoviePartyInvite } from "../invites/deepLinks";
import { CinemaView } from "../views/CinemaView";
import { CreatePartyView, type CreatePartyRequest } from "../views/CreatePartyView";
import { EndPartyConfirmView } from "../views/EndPartyConfirmView";
import { HomeView } from "../views/HomeView";
import { JoinPartyView } from "../views/JoinPartyView";
import { LobbyView } from "../views/LobbyView";
import { ReadyCheckView } from "../views/ReadyCheckView";
import { PartnerConnectView } from "../views/PartnerConnectView";
import { TailscaleSetupView } from "../views/TailscaleSetupView";
import { createCallTileSessionState, type CallTileSessionState } from "../overlays/callTileState";
import { LoadingState } from "./LoadingState";
import { useCallback, useEffect, useRef, useState } from "react";

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
  const [joinFailureCode, setJoinFailureCode] = useState<string | null>(null);
  const [providerCapabilities, setProviderCapabilities] = useState<ProviderCapability[]>([]);
  const [pendingInvite, setPendingInvite] = useState("");
  const [devScreen, setDevScreen] = useState<DevScreen>(null);
  const [tailscaleReadiness, setTailscaleReadiness] = useState<TailscaleReadiness | null>(null);
  const [isRefreshingConnectivity, setIsRefreshingConnectivity] = useState(false);
  const connectivityInFlight = useRef(false);
  const [callTileSession, setCallTileSession] = useState<CallTileSessionState>(() =>
    createCallTileSessionState(),
  );

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

  const applySnapshot = useCallback(
    (next: AppSnapshot | null) => {
      if (next) {
        setSnapshot(next);
      }
    },
    [setSnapshot],
  );

  const openJoinWithInvite = useCallback(
    (rawInvite: string) => {
      const parsed = parseMoviePartyInvite(rawInvite);
      setDevScreen(null);
      setLocalScreen(null);
      setCallTileSession(createCallTileSessionState());

      if (!parsed.ok) {
        setPendingInvite(rawInvite.trim());
        setJoinError(parsed.message);
        setJoinFailureCode(null);
        void showJoinParty().then(applySnapshot);
        return;
      }

      setPendingInvite(parsed.invite);
      setJoinError(null);
      setJoinFailureCode(null);
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

  const refreshConnectivity = useCallback(async () => {
    if (connectivityInFlight.current) return;
    connectivityInFlight.current = true;
    setIsRefreshingConnectivity(true);
    try {
      setTailscaleReadiness(await getTailscaleReadiness());
    } finally {
      connectivityInFlight.current = false;
      setIsRefreshingConnectivity(false);
    }
  }, []);

  useEffect(() => {
    void refreshConnectivity();
  }, [refreshConnectivity]);

  const isTailscaleReady = tailscaleReadiness?.state === "READY";

  useEffect(() => {
    // Poll connectivity continuously. A slower cadence while READY keeps the
    // setup gate truthful for READY → STOPPED transitions without hammering
    // the backend; a fast cadence while not READY lets the app unlock the
    // moment Tailscale becomes usable.
    const interval = setInterval(
      () => {
        void refreshConnectivity();
      },
      isTailscaleReady ? 15_000 : 4_000,
    );
    return () => {
      clearInterval(interval);
    };
  }, [isTailscaleReady, refreshConnectivity]);

  const goHome = () => {
    setDevScreen(null);
    setLocalScreen(null);
    setCreateError(null);
    setJoinError(null);
    setJoinFailureCode(null);
    setPendingInvite("");
    setCallTileSession(createCallTileSessionState());
    void showHome().then(applySnapshot);
  };

  const goCreateParty = () => {
    setDevScreen(null);
    setCreateError(null);
    setCallTileSession(createCallTileSessionState());
    setLocalScreen("CREATE_PARTY");
  };

  const createParty = async (request: CreatePartyRequest): Promise<boolean> => {
    const source =
      request.source === "local" ? (request.mediaPath?.trim() ?? "") : request.url.trim();
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
          setCreateError(partySnapshot?.error ?? "Movie Party could not create the room.");
          return false;
        }

        const providerSnapshot = await launchProvider(request.providerId, source, request.mode);
        if (!providerSnapshot || providerSnapshot.error) {
          applySnapshot(partySnapshot);
          setCreateError(providerSnapshot?.error ?? "Movie Party could not prepare that provider.");
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
          setCreateError(partySnapshot?.error ?? "Movie Party could not create the room.");
          return false;
        }

        const linkSnapshot = await launchGenericLink(source);
        if (!linkSnapshot || linkSnapshot.error) {
          applySnapshot(partySnapshot);
          setCreateError(linkSnapshot?.error ?? "Movie Party could not prepare that link.");
          return false;
        }

        applySnapshot(linkSnapshot);
        setLocalScreen(null);
        return true;
      }

      const next = await createLocalParty(source);
      if (next?.screen !== "LOBBY") {
        applySnapshot(next);
        setCreateError(next?.error ?? "Movie Party could not create the room.");
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

  const openProvider = async (providerId: string): Promise<boolean> => {
    setCreateError(null);
    try {
      const next = await openProviderBrowser(providerId);
      if (!next || next.error) {
        setCreateError(next?.error ?? "Movie Party could not open that provider.");
        return false;
      }
      applySnapshot(next);
      return true;
    } catch (error) {
      setCreateError(createRoomErrorMessage(error));
      return false;
    }
  };

  const checkProvider = async (providerId: string): Promise<boolean> => {
    setCreateError(null);
    try {
      const next = await checkProviderStatus(providerId);
      if (!next || next.error) {
        setCreateError(next?.error ?? "Movie Party could not check that provider.");
        return false;
      }
      applySnapshot(next);
      return true;
    } catch (error) {
      setCreateError(createRoomErrorMessage(error));
      return false;
    }
  };

  const openProviderTitle = async (providerId: string, title: string): Promise<boolean> => {
    setCreateError(null);
    try {
      const next = await navigateProviderTitle(providerId, title);
      if (!next || next.error) {
        setCreateError(next?.error ?? "Movie Party could not open that title.");
        return false;
      }
      applySnapshot(next);
      return true;
    } catch (error) {
      setCreateError(createRoomErrorMessage(error));
      return false;
    }
  };

  const goJoinParty = () => {
    setDevScreen(null);
    setLocalScreen(null);
    setJoinError(null);
    setJoinFailureCode(null);
    setPendingInvite("");
    setCallTileSession(createCallTileSessionState());
    void showJoinParty().then(applySnapshot);
  };

  const submitJoin = async (inviteCode: string): Promise<boolean> => {
    const parsed = parseMoviePartyInvite(inviteCode);
    if (!parsed.ok) {
      setJoinError(parsed.message);
      setJoinFailureCode(null);
      return false;
    }

    setPendingInvite(parsed.invite);
    setJoinError(null);
    setJoinFailureCode(null);
    setIsJoining(true);
    try {
      const next = await joinParty(parsed.invite);
      if (next?.screen !== "LOBBY") {
        applySnapshot(next);
        setJoinError(next?.error ?? "Movie Party could not join that invite.");
        return false;
      }

      applySnapshot(next);
      return true;
    } catch (error) {
      setJoinFailureCode(error instanceof BackendCommandError ? error.code : null);
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
      <LoadingState title="Starting Movie Party" message="Connecting to the local app runtime." />
    );
  }

  if (!tailscaleReadiness) {
    return (
      <LoadingState
        title="Checking private connection"
        message="Verifying Tailscale on this device."
      />
    );
  }

  if (tailscaleReadiness.state !== "READY") {
    return (
      <TailscaleSetupView
        readiness={tailscaleReadiness}
        isRefreshing={isRefreshingConnectivity}
        onRefresh={() => void refreshConnectivity()}
        onOpenSetup={(action) => {
          void openTailscaleSetup(action);
        }}
      />
    );
  }

  if (joinFailureCode === "MP-NET-TS-005") {
    return (
      <PartnerConnectView
        isRetrying={isJoining}
        onRetry={() => {
          if (pendingInvite) {
            void submitJoin(pendingInvite);
          }
        }}
        onOpenHelp={() => {
          void openTailscaleSetup("PARTNER_HELP");
        }}
      />
    );
  }

  if (devScreen === "HOME") return <HomeView onCreate={goCreateParty} onJoin={goJoinParty} />;

  if (localScreen === "CREATE_PARTY" || devScreen === "CREATE") {
    return (
      <CreatePartyView
        isCreating={isCreating}
        error={createError ?? snapshot.error}
        providerCapabilities={providerCapabilities}
        provider={snapshot.provider}
        onBack={goHome}
        onCreateParty={createParty}
        onOpenProvider={openProvider}
        onCheckProviderStatus={checkProvider}
        onNavigateProviderTitle={openProviderTitle}
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
        callTileSession={callTileSession}
        onCallTileSessionChange={setCallTileSession}
      />
    );
  }

  if (snapshot.screen === "READY_CHECK" || devScreen === "READY") {
    return <ReadyCheckView snapshot={snapshot} onStart={goCinema} />;
  }

  if (snapshot.screen === "CINEMA" || devScreen === "CINEMA") {
    return (
      <CinemaView
        snapshot={snapshot}
        onSnapshot={setSnapshot}
        onLeave={requestPartyEnd}
        callTileSession={callTileSession}
        onCallTileSessionChange={setCallTileSession}
      />
    );
  }

  if (snapshot.screen === "PARTY_END_CONFIRM") {
    return <EndPartyConfirmView onCancel={goCinema} onConfirm={confirmEndParty} />;
  }

  return <HomeView onCreate={goCreateParty} onJoin={goJoinParty} />;
}

function createRoomErrorMessage(error: unknown): string {
  const fallback = "Movie Party could not create the room.";
  if (error instanceof BackendCommandError) {
    if (error.code === "MP-NET-TS-001")
      return "Install Tailscale before creating a private cinema.";
    if (error.code === "MP-NET-TS-002")
      return "Sign in to Tailscale before creating a private cinema.";
    if (error.code === "MP-NET-TS-003")
      return "Tailscale is not running on this device. Open the Tailscale app and start it.";
    if (error.code === "MP-NET-TS-004")
      return "Tailscale is connected but has no private address Movie Party can use. Check its network.";
    if (error.code === "MP-NET-TS-006")
      return "Tailscale is off. Open the Tailscale app and turn it on.";
  }
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
      if (error.code === "MP-NET-TS-005") {
        return "MP-NET-TS-005 host is not reachable through Tailscale";
      }
      return "Movie Party could not reach the host. Check that both devices are online and connected to Tailscale.";
    }
    return "Movie Party could not join that room right now.";
  }

  return "Movie Party could not join that invite.";
}
