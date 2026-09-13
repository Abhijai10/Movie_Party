import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import {
  ArrowLeft,
  Check,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  UserPlus,
  Users,
  X,
} from "lucide-react";
import { CinemaButton } from "../components/mp/CinemaButton";
import { InviteCard } from "../components/mp/InviteCard";
import { SilkBackground } from "../components/mp/SilkBackground";
import { StatusIndicator } from "../components/mp/StatusIndicator";
import {
  acceptFriendInvite,
  friendErrorCopy,
  friendInviteLink,
  friendStatusCopy,
  friendStatusFor,
  openTailscaleSetup,
  refreshFriendStates,
  removeFriend,
  renameFriend,
  verifyFriend,
  type FriendInviteLink,
  type StoredFriend,
} from "../backend/appRuntime";
import { parseFriendInvite } from "../invites/deepLinks";

/**
 * Friends — the invite-link surface of the Tailscale friend architecture:
 *
 *   Movie Party friend invite (identity-only link — NEVER an auth key)
 *   → friend accepts it here (INVITED)
 *   → their device joins the tailnet through Tailscale's own
 *     external-user invitation with THEIR Tailscale identity
 *   → Movie Party refreshes the tailnet status (TAILSCALE_JOINED)
 *   → Movie Party verifies the expected peer with a real ping
 *     (MOVIE_PARTY_VERIFIED → ONLINE/OFFLINE as a live observation).
 *
 * No tailnet jargon on the surface: share a link/QR, your friend opens
 * it, and they land in this list under an editable name with honest
 * states.
 */
type FriendsViewProps = {
  onBack: () => void;
  friends: StoredFriend[];
  onFriendsChanged: (friends: StoredFriend[]) => void;
  /** A friend invite that arrived via deep link — accepted on mount. */
  pendingInvite: string | null;
  onPendingInviteConsumed: () => void;
};

export function FriendsView({
  onBack,
  friends,
  onFriendsChanged,
  pendingInvite,
  onPendingInviteConsumed,
}: FriendsViewProps) {
  const [invite, setInvite] = useState<FriendInviteLink | null>(null);
  const [inviteError, setInviteError] = useState<string | null>(null);
  const [pasteValue, setPasteValue] = useState("");
  const [pasteError, setPasteError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<"verify" | "remove" | "rename" | "add" | null>(
    null,
  );
  const [renamingKey, setRenamingKey] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      // refresh_friend_states promotes INVITED friends whose device has
      // since joined the tailnet (TAILSCALE_JOINED) and never demotes a
      // verified friend — the observation step of the friend flow.
      onFriendsChanged(await refreshFriendStates());
    } catch {
      // list_friends never throws in Rust — keep the last known list.
    }
  }, [onFriendsChanged]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (invite != null || inviteError != null) return;
    let cancelled = false;
    void friendInviteLink().then(
      (link) => {
        if (!cancelled) setInvite(link);
      },
      (error: unknown) => {
        if (!cancelled) {
          setInviteError(friendErrorCopy(error, "Movie Party couldn't create your friend link."));
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, [invite, inviteError]);

  const acceptInvite = useCallback(
    async (raw: string) => {
      const parsed = parseFriendInvite(raw);
      if (!parsed.ok) {
        setPasteError(parsed.message);
        return false;
      }
      setPasteError(null);
      setBusyAction("add");
      try {
        const friend = await acceptFriendInvite(raw);
        await refresh();
        setPasteValue("");
        setNotice(
          `${friend.displayName} is now in your friends. Next: join their private network in the Tailscale app (their Tailscale invite) — then come back and Connect.`,
        );
        return true;
      } catch (error) {
        setPasteError(friendErrorCopy(error, "Movie Party couldn't add that friend."));
        return false;
      } finally {
        setBusyAction(null);
      }
    },
    [refresh],
  );

  // A deep-link invite (QR scan / link open) is accepted on arrival.
  useEffect(() => {
    if (pendingInvite == null) return;
    void (async () => {
      await acceptInvite(pendingInvite);
      onPendingInviteConsumed();
    })();
  }, [pendingInvite, acceptInvite, onPendingInviteConsumed]);

  const doVerify = async (peerKey: string) => {
    setActionError(null);
    setNotice(null);
    setBusyKey(peerKey);
    setBusyAction("verify");
    try {
      const { probe } = await verifyFriend(peerKey);
      await refresh();
      if (probe.reachable) {
        setNotice("Connection verified — the tunnel answered.");
      } else {
        setActionError(friendErrorCopy(probe.message, "The connection check didn't get an answer."));
      }
    } catch (error) {
      setActionError(friendErrorCopy(error, "The connection check failed."));
    } finally {
      setBusyKey(null);
      setBusyAction(null);
    }
  };

  const doRemove = async (peerKey: string, name: string) => {
    setActionError(null);
    setBusyKey(peerKey);
    setBusyAction("remove");
    try {
      await removeFriend(peerKey);
      await refresh();
      setNotice(`${name} was removed.`);
    } catch (error) {
      setActionError(friendErrorCopy(error, "Movie Party couldn't remove that friend."));
    } finally {
      setBusyKey(null);
      setBusyAction(null);
    }
  };

  const saveRename = async (peerKey: string) => {
    setActionError(null);
    setBusyKey(peerKey);
    setBusyAction("rename");
    try {
      await renameFriend(peerKey, renameValue.trim());
      setRenamingKey(null);
      await refresh();
    } catch (error) {
      setActionError(friendErrorCopy(error, "Movie Party couldn't rename that friend."));
    } finally {
      setBusyKey(null);
      setBusyAction(null);
    }
  };

  return (
    <div className="relative w-full min-h-screen" data-testid="friends-screen">
      <div className="fixed inset-0 pointer-events-none">
        <SilkBackground variant="calm" />
      </div>

      <header className="relative z-10 flex items-center justify-between px-12 pt-8">
        <button
          type="button"
          onClick={onBack}
          className="flex items-center gap-2 text-white/60 hover:text-white transition text-sm tracking-wider"
          data-testid="friends-back-btn"
        >
          <ArrowLeft className="w-4 h-4" strokeWidth={1.6} /> Back
        </button>
        <StatusIndicator state="sync" label="Strict Sync" />
      </header>

      <main className="relative z-10 max-w-[1100px] w-full mx-auto px-12 mt-6 pb-20">
        <motion.section
          initial={{ opacity: 0, y: 14 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
        >
          <h1 className="font-serif-display text-white text-4xl tracking-[-0.02em]">Friends</h1>
          <p className="mt-3 text-white/50 text-sm max-w-lg leading-relaxed">
            Share your link — your friend scans the QR or opens it on a device with Movie Party,
            and they appear here under a name you can edit. No addresses, no tailnet jargon.
          </p>

          {notice != null ? (
            <p role="status" data-testid="friends-notice" className="mt-4 text-sm text-emerald-300/85">
              {notice}
            </p>
          ) : null}
          {actionError != null ? (
            <p role="alert" data-testid="friends-action-error" className="mt-4 text-sm text-[#FCA5A5]">
              {actionError}
            </p>
          ) : null}

          <div className="mt-8 friends-layout" data-testid="friends-grid">
            <div className="space-y-6">
              {invite != null ? (
                <div className="space-y-3 min-w-0 friends-card" data-testid="friends-invite-block">
                  <InviteCard inviteCode={invite.link} title="Your friend link" qrHint="Scan to add you" />
                  <p className="text-xs text-white/45 leading-relaxed">
                    The link carries only your name — never a network key. Your friend joins
                    your Tailscale network on their device (Tailscale asks them to sign in
                    with their own account), then Movie Party sees and verifies them here.
                  </p>
                </div>
              ) : inviteError != null ? (
                <div
                  className="rounded-2xl border border-amber-400/25 bg-amber-400/[0.06] p-5"
                  data-testid="friends-invite-error"
                >
                  <p className="text-sm text-amber-100/90">{inviteError}</p>
                </div>
              ) : (
                <div
                  className="rounded-2xl border border-white/10 bg-white/[0.03] p-5 text-sm text-white/50"
                  data-testid="friends-invite-loading"
                >
                  Creating your friend link…
                </div>
              )}

              <div
                className="rounded-2xl border border-white/10 bg-white/[0.03] p-5 friends-card"
                data-testid="friends-add-from-link"
              >
                <span className="flex items-center gap-2 text-[11px] tracking-[0.28em] uppercase text-white/50">
                  <UserPlus className="w-3.5 h-3.5 shrink-0" /> Have a friend's link?
                </span>
                <div className="mt-3 flex flex-wrap gap-3 min-w-0">
                  <input
                    value={pasteValue}
                    onChange={(event) => {
                      setPasteValue(event.target.value);
                    }}
                    placeholder="Paste movieparty://friend/…"
                    data-testid="friends-paste-input"
                    className={`mp-input font-mono-mp${
                      pasteValue.trim().length > 0 ? " mp-input--filled" : ""
                    }`}
                  />
                  <CinemaButton
                    onClick={() => {
                      void acceptInvite(pasteValue);
                    }}
                    disabled={busyAction === "add" || pasteValue.trim().length === 0}
                    icon={busyAction === "add" ? undefined : Plus}
                    data-testid="friends-paste-add-btn"
                  >
                    {busyAction === "add" ? "Adding…" : "Add"}
                  </CinemaButton>
                </div>
                {pasteError != null ? (
                  <p role="alert" className="mt-3 text-sm text-[#FCA5A5]" data-testid="friends-paste-error">
                    {pasteError}
                  </p>
                ) : null}
              </div>
            </div>

            <div
              className="rounded-2xl border border-white/10 bg-white/[0.03] p-5 friends-card"
              data-testid="friends-list-card"
            >
              <div className="flex items-center justify-between">
                <span className="flex items-center gap-2 text-[11px] tracking-[0.28em] uppercase text-white/50">
                  <Users className="w-3.5 h-3.5" /> Your friends
                </span>
                <span className="text-[10px] tracking-widest uppercase text-white/35">
                  {String(friends.length)} saved
                </span>
              </div>

              {friends.length === 0 ? (
                <p className="mt-4 text-sm leading-relaxed text-white/45" data-testid="friends-empty">
                  No friends yet. Share your link above — once they scan it, they'll appear here
                  with a name you can edit.
                </p>
              ) : (
                <ul className="mt-4 space-y-3 friends-scroll">
                  {friends.map((friend) => {
                    const status = friendStatusFor(friend, undefined);
                    const renaming = renamingKey === friend.peerKey;
                    const busy = busyKey === friend.peerKey;
                    return (
                      <li
                        key={friend.peerKey}
                        className="rounded-xl border border-white/10 bg-white/[0.02] px-4 py-3.5"
                        data-testid={`friend-row-${friend.peerKey}`}
                      >
                        {renaming ? (
                          <div className="flex items-center gap-2">
                            <input
                              value={renameValue}
                              onChange={(event) => {
                                setRenameValue(event.target.value);
                              }}
                              aria-label="Friend name"
                              data-testid={`friend-rename-input-${friend.peerKey}`}
                              className="mp-input font-mono-mp"
                            />
                            <button
                              type="button"
                              disabled={busy}
                              onClick={() => {
                                void saveRename(friend.peerKey);
                              }}
                              className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-white/5 hover:bg-white/10 border border-white/10 transition text-[10px] tracking-widest uppercase text-white/70 disabled:opacity-40"
                              data-testid={`friend-rename-save-${friend.peerKey}`}
                            >
                              {busy && busyAction === "rename" ? (
                                <Loader2 className="w-3 h-3 animate-spin" />
                              ) : (
                                <Check className="w-3 h-3" />
                              )}
                              Save
                            </button>
                            <button
                              type="button"
                              onClick={() => {
                                setRenamingKey(null);
                              }}
                              className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-white/5 hover:bg-white/10 border border-white/10 transition text-[10px] tracking-widest uppercase text-white/50"
                              data-testid={`friend-rename-cancel-${friend.peerKey}`}
                            >
                              <X className="w-3 h-3" /> Cancel
                            </button>
                          </div>
                        ) : (
                          <div className="min-w-0">
                            <div className="flex items-center justify-between gap-3">
                              <div className="flex items-center gap-2 min-w-0">
                                <p className="text-sm text-white/85 truncate">{friend.displayName}</p>
                                <button
                                  type="button"
                                  onClick={() => {
                                    setRenamingKey(friend.peerKey);
                                    setRenameValue(friend.displayName);
                                  }}
                                  title="Edit name"
                                  aria-label={`Rename ${friend.displayName}`}
                                  className="text-white/35 hover:text-white transition shrink-0"
                                  data-testid={`friend-rename-btn-${friend.peerKey}`}
                                >
                                  <Pencil className="w-3.5 h-3.5" />
                                </button>
                              </div>
                              <div className="flex items-center gap-2 shrink-0 friends-row-actions">
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => {
                                    void doVerify(friend.peerKey);
                                  }}
                                  title="Verify the real tunnel connection"
                                  className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-white/5 hover:bg-white/10 border border-white/10 transition text-[10px] tracking-widest uppercase text-white/70 disabled:opacity-40"
                                  data-testid={`friend-verify-btn-${friend.peerKey}`}
                                >
                                  {busy && busyAction === "verify" ? (
                                    <Loader2 className="w-3 h-3 animate-spin" />
                                  ) : (
                                    <RefreshCw className="w-3 h-3" />
                                  )}
                                  {busy && busyAction === "verify"
                                    ? "Pinging"
                                    : status === "MOVIE_PARTY_VERIFIED"
                                      ? "Re-verify"
                                      : "Verify"}
                                </button>
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => {
                                    void doRemove(friend.peerKey, friend.displayName);
                                  }}
                                  title="Remove friend"
                                  aria-label={`Remove ${friend.displayName}`}
                                  className="flex items-center px-2.5 py-1.5 rounded-full bg-white/5 hover:bg-white/10 border border-white/10 transition text-white/40 hover:text-[#FCA5A5] disabled:opacity-40"
                                  data-testid={`friend-remove-btn-${friend.peerKey}`}
                                >
                                  {busy && busyAction === "remove" ? (
                                    <Loader2 className="w-3 h-3 animate-spin" />
                                  ) : (
                                    <Trash2 className="w-3 h-3" />
                                  )}
                                </button>
                              </div>
                            </div>
                            <p className="mt-1 text-[11px] text-white/45">
                              {friendStatusCopy(friend, status)}
                            </p>
                            {status === "TAILSCALE_PENDING" ? (
                              <div
                                className="mt-2.5 rounded-lg border border-amber-400/25 bg-amber-400/[0.05] px-3.5 py-3"
                                data-testid={`friend-pending-help-${friend.peerKey}`}
                              >
                                <p className="text-[11px] leading-relaxed text-amber-100/80">
                                  <strong className="text-amber-200/90">Two separate steps.</strong>{" "}
                                  {friend.displayName} accepted your Movie Party invite. For
                                  Movie Party to reach their device, they must also join your
                                  private network themselves in the Tailscale app — using the
                                  Tailscale invite your network sends them (email or invite link),
                                  not this app. Movie Party cannot and will never join for them.
                                </p>
                                <div className="mt-2.5 flex flex-wrap items-center gap-2.5">
                                  <button
                                    type="button"
                                    onClick={() => {
                                      void openTailscaleSetup("OPEN_APP");
                                    }}
                                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-full border border-amber-300/30 bg-amber-300/10 hover:bg-amber-300/20 transition text-[10px] tracking-widest uppercase text-amber-100/90"
                                    data-testid={`friend-open-tailscale-${friend.peerKey}`}
                                  >
                                    Open Tailscale
                                  </button>
                                  <span className="text-[10px] text-white/40">
                                    Then hit Verify to test the real connection.
                                  </span>
                                </div>
                              </div>
                            ) : null}
                          </div>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          </div>
        </motion.section>
      </main>
    </div>
  );
}
