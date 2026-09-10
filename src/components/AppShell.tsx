import {
  BackendCommandError,
  checkProviderStatus,
  commandErrorMessage,
  createLocalParty,
  listSchedules,
  type StoredSchedule,
  enterCinema,
  requestPlayCountdown,
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
  setSharedControls,
  showHome,
  showJoinParty,
  listenToDeepLinks,
  takePendingDeepLinks,
  type AppSnapshot,
  type ProviderCapability,
  type TailscaleReadiness,
  type TailscaleSetupAction,
} from "../backend/appRuntime";
import { useAppSnapshot } from "../hooks/useAppSnapshot";
import { parseMoviePartyInvite } from "../invites/deepLinks";
import { GuestScheduleAccept } from "./mp/GuestScheduleAccept";
import { RetentionPrompt } from "./mp/RetentionPrompt";
import { CinemaView } from "../views/CinemaView";
import { DebugHud } from "./DebugHud";
import { FirstRunView } from "../views/FirstRunView";
import { ScheduleView } from "../views/ScheduleView";
import { SettingsView } from "../views/SettingsView";
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
import { handleFailureEvent } from "../backend/appRuntime";
import {
  createSingleFlight,
  isTailscaleReady,
  pollIntervalMs,
  shouldShowPartnerConnectView,
} from "../backend/tailscaleOnboarding";

type LocalScreen = "CREATE_PARTY" | "SETTINGS" | "SCHEDULE" | "FIRST_RUN" | null;

/** §69: window-close-during-party state. */
type ClosePromptState = { visible: boolean; isHost: boolean };
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
  const [setupOpenError, setSetupOpenError] = useState<string | null>(null);
  const refreshConnectivityOnce = useRef(createSingleFlight());
  const [callTileSession, setCallTileSession] = useState<CallTileSessionState>(() =>
    createCallTileSessionState(),
  );
  // ── state ──────────────────────────────────────────────────
  const [upcoming, setUpcoming] = useState<StoredSchedule[]>([]);
  const [preloadProgress, setPreloadProgress] = useState<Record<string, number>>({});
  const [firstRunDone, setFirstRunDone] = useState<boolean>(() => {
    try {
      return localStorage.getItem("mp_first_run_complete") === "1";
    } catch {
      return true; // storage unavailable → never block the app
    }
  });
  const [closePrompt, setClosePrompt] = useState<ClosePromptState>({
    visible: false,
    isHost: false,
  });
  /** Media filenames seen in snapshots, by media id — for Upcoming cards. */
  const mediaNameCache = useRef<Record<string, string>>({});
  /** §56: schedule ids the guest already answered (accepted or declined)
   *  — the banner never re-prompts for the same schedule. */
  const [answeredSchedules, setAnsweredSchedules] = useState<Set<string>>(new Set());
  const debugHudEnabled =
    developmentPreviewEnabled &&
    typeof window !== "undefined" &&
    new URLSearchParams(window.location.search).has("debug");

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

  // (§10/§11): First Run shows until the user continues once.
  useEffect(() => {
    if (!firstRunDone) {
      setLocalScreen("FIRST_RUN");
    }
  }, [firstRunDone]);

  // (§53): refresh upcoming schedules when home is visible.
  const isHomeVisible =
    snapshot === null ||
    (snapshot.screen === "HOME" && localScreen === null && devScreen === null);
  useEffect(() => {
    if (!isHomeVisible) return;
    let cancelled = false;
    void listSchedules().then((schedules) => {
      if (!cancelled) {
        setUpcoming(
          schedules.filter(
            (schedule) =>
              schedule.status !== "Cancelled" &&
              schedule.status !== "Completed" &&
              schedule.scheduledStartUtcMs > Date.now() - 24 * 3600_000,
          ),
        );
      }
    });
    return () => {
      cancelled = true;
    };
  }, [isHomeVisible]);

  // Media filenames by id — feeds the Upcoming cards' honest titles.
  useEffect(() => {
    if (snapshot?.media) {
      mediaNameCache.current[snapshot.media.mediaId] = snapshot.media.filename;
    }
  }, [snapshot?.media]);

  // (§57): PRELOAD_STATE progress arrives via snapshots' transfer
  // when the schedule's media is the active transfer. Map it onto cards.
  useEffect(() => {
    if (snapshot?.transfer && snapshot.media) {
      const mediaId = snapshot.media.mediaId;
      const percent = snapshot.transfer.bytesTotal
        ? snapshot.transfer.bytesAvailable / snapshot.transfer.bytesTotal
        : 0;
      setPreloadProgress((previous) => {
        const next = { ...previous };
        for (const schedule of upcoming) {
          if (schedule.mediaId === mediaId) {
            next[schedule.scheduleId] = percent;
          }
        }
        return next;
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snapshot?.transfer?.bytesAvailable, snapshot?.media?.mediaId]);

  // §69: window-close-during-party prompt. In a party, closing asks first —
  // Leave vs End-For-Everyone (host) vs just Leave (guest). The Tauri
  // window can't be intercepted from the renderer, so this fires on the
  // app's own close affordances; the OS-level close is handled by the
  // backend's graceful teardown (earlier call work).
  useEffect(() => {
    const inParty =
      snapshot?.screen === "CINEMA" ||
      snapshot?.screen === "LOBBY" ||
      snapshot?.screen === "READY_CHECK";
    if (inParty && !closePrompt.visible) {
      const beforeUnload = (event: BeforeUnloadEvent) => {
        event.preventDefault();
      };
      window.addEventListener("beforeunload", beforeUnload);
      return () => {
        window.removeEventListener("beforeunload", beforeUnload);
      };
    }
    return undefined;
  }, [snapshot?.screen, closePrompt.visible]);

  // ── sleep/wake + network-change revalidation hooks ────
  // SLEEP_WAKE: the OS hiding the window (lid close, sleep) followed by a
  // visible return after a real gap means clocks/buffers/devices must be
  // revalidated — the modeled plan pauses playback for both and rechecks
  // (§14: never risk desync on stale state).
  const lastHiddenAtRef = useRef<number | null>(null);
  useEffect(() => {
    const inParty =
      snapshot?.screen === "CINEMA" ||
      snapshot?.screen === "LOBBY" ||
      snapshot?.screen === "READY_CHECK";
    if (!inParty) {
      return undefined;
    }
    const onVisibility = () => {
      if (document.visibilityState === "hidden") {
        lastHiddenAtRef.current = Date.now();
        return;
      }
      const hiddenAt = lastHiddenAtRef.current;
      lastHiddenAtRef.current = null;
      // Only a real gap (≥ 5 s) is a sleep/wake — quick app switches are
      // not; firing SLEEP_WAKE for those would pointlessly pause both.
      if (hiddenAt != null && Date.now() - hiddenAt >= 5_000) {
        void handleFailureEvent("SLEEP_WAKE").then(applySnapshot);
      }
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [applySnapshot, snapshot?.screen]);

  // NETWORK_CHANGE / WIFI_DISCONNECT: browser online/offline events map to
  // the modeled network revalidation plans (revalidate + rebuild buffers
  // on recovery; pause both on loss).
  useEffect(() => {
    const inParty =
      snapshot?.screen === "CINEMA" ||
      snapshot?.screen === "LOBBY" ||
      snapshot?.screen === "READY_CHECK";
    if (!inParty) {
      return undefined;
    }
    const onOnline = () => {
      void handleFailureEvent("NETWORK_CHANGE").then(applySnapshot);
    };
    const onOffline = () => {
      void handleFailureEvent("WIFI_DISCONNECT").then(applySnapshot);
    };
    window.addEventListener("online", onOnline);
    window.addEventListener("offline", onOffline);
    return () => {
      window.removeEventListener("online", onOnline);
      window.removeEventListener("offline", onOffline);
    };
  }, [applySnapshot, snapshot?.screen]);

  const requestLeaveOrEnd = useCallback(() => {
    const isHost = snapshot?.room.role === "HOST";
    setClosePrompt({ visible: true, isHost });
  }, [snapshot?.room.role]);

  const completeFirstRun = useCallback(() => {
    try {
      localStorage.setItem("mp_first_run_complete", "1");
    } catch {
      /* storage unavailable — the session flag still clears */
    }
    setFirstRunDone(true);
    setLocalScreen(null);
  }, []);

  const refreshConnectivity = useCallback(async () => {
    await refreshConnectivityOnce.current(async () => {
      setIsRefreshingConnectivity(true);
      try {
        setTailscaleReadiness(await getTailscaleReadiness());
      } finally {
        setIsRefreshingConnectivity(false);
      }
    });
  }, []);

  const openSetup = useCallback(
    (action: TailscaleSetupAction) => {
      void openTailscaleSetup(action).then((message) => {
        setSetupOpenError(message);
      });
    },
    [],
  );

  useEffect(() => {
    void refreshConnectivity();
  }, [refreshConnectivity]);

  const tailscaleReady =
    tailscaleReadiness !== null && isTailscaleReady(tailscaleReadiness.state);

  useEffect(() => {
    // Poll connectivity continuously. A slower cadence while READY keeps the
    // setup gate truthful for READY → STOPPED transitions without hammering
    // the backend; a fast cadence while not READY lets the app unlock the
    // moment Tailscale becomes usable.
    const interval = setInterval(
      () => {
        void refreshConnectivity();
      },
      pollIntervalMs(tailscaleReady),
    );
    return () => {
      clearInterval(interval);
    };
  }, [tailscaleReady, refreshConnectivity]);

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

  // navigation helpers.
  const goSettings = () => {
    setDevScreen(null);
    setLocalScreen("SETTINGS");
  };

  const goSchedule = () => {
    setDevScreen(null);
    setLocalScreen("SCHEDULE");
  };

  // Media naming for Upcoming cards — the snapshot's live media when it
  // matches, else a stored cache entry name, else the honest raw id.
  // §56: the guest's pending schedule accept — shown until answered.
  const pendingGuestSchedule =
    snapshot?.pendingGuestSchedule != null &&
    !answeredSchedules.has(snapshot.pendingGuestSchedule.scheduleId)
      ? snapshot.pendingGuestSchedule
      : null;

  // §52: the post-party retention question (guest with transferred media).
  const retentionPrompt = snapshot?.retentionPrompt ?? null;

  const mediaNameFor = (mediaId: string): string => {
    if (snapshot?.media?.mediaId === mediaId) {
      return snapshot.media.filename;
    }
    return mediaNameCache.current[mediaId] ?? mediaId;
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

  if (!tailscaleReady) {
    return (
      <TailscaleSetupView
        readiness={tailscaleReadiness}
        isRefreshing={isRefreshingConnectivity}
        openError={setupOpenError}
        onRefresh={() => {
          setSetupOpenError(null);
          void refreshConnectivity();
        }}
        onOpenSetup={openSetup}
      />
    );
  }

  if (shouldShowPartnerConnectView(joinFailureCode)) {
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

  if (devScreen === "HOME")
    return (
      <>
        <HomeView
          onCreate={goCreateParty}
          onJoin={goJoinParty}
          upcoming={upcoming}
          preloadProgress={preloadProgress}
          mediaNameById={mediaNameFor}
          onOpenSettings={goSettings}
          onOpenSchedule={goSchedule}
        />
        {debugHudEnabled ? <DebugHud snapshot={snapshot} /> : null}
      </>
    );

  // (§10/§11): First Run — one-time welcome + truthful checks.
  if (localScreen === "FIRST_RUN") {
    return <FirstRunView onContinue={completeFirstRun} />;
  }

  // (§55–§62): Settings — always reachable from the shell.
  if (localScreen === "SETTINGS") {
    return (
      <>
        <SettingsView snapshot={snapshot} onBack={goHome} />
        {debugHudEnabled ? <DebugHud snapshot={snapshot} /> : null}
      </>
    );
  }

  // (§18/§19): Schedule form.
  if (localScreen === "SCHEDULE") {
    return (
      <>
        <ScheduleView
          snapshot={snapshot}
          onBack={goHome}
          onScheduled={() => {
            setLocalScreen(null);
            void listSchedules().then((schedules) => {
              setUpcoming(schedules);
            });
          }}
        />
        {debugHudEnabled ? <DebugHud snapshot={snapshot} /> : null}
      </>
    );
  }

  // §69: window-close-during-party — host decides End-For-Everyone vs Leave.
  if (closePrompt.visible) {
    return (
      <EndPartyConfirmView
        onCancel={() => {
          setClosePrompt({ visible: false, isHost: false });
        }}
        onConfirm={() => {
          setClosePrompt({ visible: false, isHost: false });
          void leaveParty().then((next) => {
            applySnapshot(next);
          });
        }}
      />
    );
  }

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
      <>
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
        {pendingGuestSchedule != null ? (
          <GuestScheduleAccept
            scheduleId={pendingGuestSchedule.scheduleId}
            scheduledStartUtcMs={pendingGuestSchedule.scheduledStartUtcMs}
            onAnswered={() => {
              setAnsweredSchedules((current) => {
                const next = new Set(current);
                next.add(pendingGuestSchedule.scheduleId);
                return next;
              });
            }}
          />
        ) : null}
      </>
    );
  }

  if (snapshot.screen === "READY_CHECK" || devScreen === "READY") {
    return (
      <ReadyCheckView
        snapshot={snapshot}
        onRequestCountdown={() => {
          void requestPlayCountdown().then(applySnapshot);
        }}
        onStarted={goCinema}
        callTileSession={callTileSession}
        onCallTileSessionChange={setCallTileSession}
      />
    );
  }

  if (snapshot.screen === "CINEMA" || devScreen === "CINEMA") {
    return (
      <CinemaView
        snapshot={snapshot}
        onSnapshot={setSnapshot}
        onLeave={requestLeaveOrEnd}
        callTileSession={callTileSession}
        onCallTileSessionChange={setCallTileSession}
      />
    );
  }

  if (snapshot.screen === "PARTY_END_CONFIRM") {
    return <EndPartyConfirmView onCancel={goCinema} onConfirm={confirmEndParty} />;
  }

  // §52: the retention question renders over the post-party HOME screen —
  // it is the one modal that legitimately interrupts because the party is
  // over and the cache decision is owed. (devScreen HOME is handled above.)
  return (
    <>
      <HomeView
        onCreate={goCreateParty}
        onJoin={goJoinParty}
        upcoming={upcoming}
        preloadProgress={preloadProgress}
        mediaNameById={mediaNameFor}
        onOpenSettings={goSettings}
        onOpenSchedule={goSchedule}
      />
      {debugHudEnabled ? <DebugHud snapshot={snapshot} /> : null}
      {retentionPrompt != null ? (
        <RetentionPrompt
          mediaId={retentionPrompt.mediaId}
          filename={retentionPrompt.filename}
          onDecided={() => {
            void showHome().then(applySnapshot);
          }}
        />
      ) : null}
    </>
  );
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
