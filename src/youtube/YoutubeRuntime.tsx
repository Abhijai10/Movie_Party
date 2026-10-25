import { unobscured } from "./visibility";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppSnapshot } from "../backend/appRuntime";
import { acceptsMessage, contextKey, type Attachment, type PlayerEvent } from "./types";

let disposeCurrent: (() => void) | null = null;
/** Removing the owned iframe stops its media synchronously before Chrome opens. */
export function stopYoutubeFrame() {
  disposeCurrent?.();
  disposeCurrent = null;
}
let handoff: { enabled: boolean; promise: Promise<AppSnapshot> } | null = null;
export function setYoutubeDriver(enabled: boolean): Promise<AppSnapshot> {
  if (handoff?.enabled === enabled) return handoff.promise;
  if (handoff) return Promise.reject(new Error("Wait for the current player handoff to finish."));
  stopYoutubeFrame();
  const promise = invoke<AppSnapshot>("youtube_driver", { enabled });
  handoff = { enabled, promise };
  void promise
    .finally(() => {
      if (handoff?.promise === promise) handoff = null;
    })
    .catch(() => undefined);
  return promise;
}

export function shouldAutoStart(
  hasSelection: boolean,
  chosen: boolean | undefined,
  allowAutomatic: boolean,
): boolean {
  return hasSelection && chosen === false && allowAutomatic;
}

export function YoutubeRuntime({
  snapshot,
  onSnapshot,
  allowAutomatic = true,
}: {
  snapshot: AppSnapshot | null;
  onSnapshot: (snapshot: AppSnapshot) => void;
  allowAutomatic?: boolean;
}) {
  const frame = useRef<HTMLIFrameElement>(null),
    current = useRef(snapshot),
    output = useRef(onSnapshot);
  current.current = snapshot;
  output.current = onSnapshot;
  const [attachment, setAttachment] = useState<Attachment | null>(null),
    [error, setError] = useState<string | null>(null);
  const [box, setBox] = useState<{
    left: number;
    top: number;
    width: number;
    height: number;
    visible: boolean;
  } | null>(null);
  const selected = snapshot?.providerContent?.selection;
  const key =
    selected?.provider === "youtube"
      ? `${snapshot?.room.roomId ?? ""}:${contextKey(selected)}`
      : null;
  const enabled = Boolean(key && snapshot?.youtube?.enabled);
  const auto = shouldAutoStart(Boolean(key), snapshot?.youtube?.chosen, allowAutomatic);
  useEffect(() => {
    if (!auto) return;
    let disposed = false;
    void setYoutubeDriver(true)
      .then((next) => {
        if (!disposed) output.current(next);
      })
      .catch(() => {
        if (!disposed)
          setError(
            "Could not hand playback to the in-app player. Retry from the YouTube controls.",
          );
      });
    return () => {
      disposed = true;
    };
  }, [auto, key]);
  useEffect(() => {
    if (!enabled || !key) return;
    let disposed = false,
      lease: Attachment | null = null,
      lastSeq = 0,
      visible = false;
    const isDisposed = () => disposed;
    let tail: Promise<unknown> = Promise.resolve();
    setError(null);
    setAttachment(null);
    stopYoutubeFrame();
    const send = (data: object) => {
      if (lease && frame.current?.contentWindow)
        frame.current.contentWindow.postMessage({ ...data, lease: lease.lease }, lease.origin);
    };
    const shutdown = () => {
      if (disposed) return;
      disposed = true;
      send({ type: "dispose" });
      frame.current?.removeAttribute("src");
      setAttachment(null);
      if (lease) void invoke("youtube_detach", { lease: lease.lease }).catch(() => undefined);
    };
    disposeCurrent = shutdown;
    void invoke<Attachment>("youtube_attach")
      .then((value) => {
        if (disposed) {
          void invoke("youtube_detach", { lease: value.lease }).catch(() => undefined);
          return;
        }
        lease = value;
        const url = new URL(value.url);
        const hash = new URLSearchParams(url.hash.slice(1));
        hash.set("parent", location.origin);
        url.hash = hash.toString();
        setAttachment({ ...value, url: url.href });
      })
      .catch(() => {
        if (!disposed) setError("Could not load the in-app player. Retry or use managed Chrome.");
      });
    const receive = (event: MessageEvent) => {
      if (
        disposed ||
        !lease ||
        !acceptsMessage(
          event,
          frame.current?.contentWindow ?? null,
          lease.origin,
          lease.lease,
          lastSeq,
        )
      )
        return;
      lastSeq = event.data.seq;
      const data: PlayerEvent = event.data;
      const selection = current.current?.providerContent?.selection;
      if (!selection || `${current.current?.room.roomId ?? ""}:${contextKey(selection)}` !== key)
        return;
      const { type: messageType, ...payload } = data;
      void messageType;
      tail = tail
        .catch(() => undefined)
        .then(async () => {
          if (disposed) return;
          try {
            const next = await invoke<AppSnapshot>("youtube_event", {
              context: {
                state_epoch: selection.state_epoch,
                host_sequence: selection.host_sequence,
                content_revision: selection.content_revision,
              },
              event: payload,
            });
            if (!isDisposed()) output.current(next);
          } catch {
            /* Unconfirmed metadata and superseded frames cannot advance readiness. */
          }
        });
    };
    window.addEventListener("message", receive);
    const timer = window.setInterval(() => {
      const node = document.querySelector<HTMLElement>("[data-youtube-slot]");
      const rect = node?.getBoundingClientRect();
      visible = Boolean(
        rect &&
        rect.width >= 200 &&
        rect.height >= 200 &&
        rect.top >= 0 &&
        rect.left >= 0 &&
        rect.bottom <= innerHeight &&
        rect.right <= innerWidth &&
        document.visibilityState === "visible" &&
        node &&
        unobscured(node),
      );
      if (rect) {
        const parent = node?.parentElement?.getBoundingClientRect();
        if (parent && (rect.top < parent.top || rect.bottom > parent.bottom)) visible = false;
        setBox({ left: rect.left, top: rect.top, width: rect.width, height: rect.height, visible });
      } else {
        setBox(null);
        visible = false;
      }
      send({ type: "visible", visible });
      const command = current.current?.youtube?.command;
      if (command && visible) send({ type: "command", ...command });
      else if (command?.action === "PAUSE") send({ type: "command", ...command });
    }, 100);
    return () => {
      clearInterval(timer);
      window.removeEventListener("message", receive);
      shutdown();
      if (disposeCurrent === shutdown) disposeCurrent = null;
    };
  }, [enabled, key]);
  return (
    <>
      {attachment && (
        <iframe
          ref={frame}
          src={attachment.url}
          title="YouTube video player"
          className="youtube-owned-frame"
          allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
          allowFullScreen
          referrerPolicy="strict-origin-when-cross-origin"
          sandbox="allow-scripts allow-same-origin allow-presentation allow-popups allow-popups-to-escape-sandbox"
          style={{
            left: box?.left ?? 0,
            top: box?.top ?? 0,
            width: box?.width ?? 200,
            height: box?.height ?? 200,
            visibility: box?.visible ? "visible" : "hidden",
          }}
        />
      )}
      {error && (
        <p className="youtube-runtime-error" role="alert">
          {error}
        </p>
      )}
    </>
  );
}
