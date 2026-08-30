import { getAppSnapshot, listenToSnapshots, type AppSnapshot } from "../backend/appRuntime";
import { type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

export function useAppSnapshot() {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);

  useEffect(() => {
    let isMounted = true;
    let unlisten: UnlistenFn | null = null;

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
      unlisten?.();
    };
  }, []);

  return { snapshot, setSnapshot };
}
