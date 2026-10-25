/* Official IFrame API only. No player DOM inspection, ad detection or auth access. */
(() => {
  const config = new URLSearchParams(location.hash.slice(1));
  const video = config.get("video"),
    lease = config.get("lease"),
    parentOrigin = config.get("parent");
  const allowedParents = [
    "null",
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost:1420",
    "http://127.0.0.1:1420",
    "http://127.0.0.1:1421",
  ];
  if (!/^[A-Za-z0-9_-]{11}$/.test(video || "") || !lease || !allowedParents.includes(parentOrigin))
    return;
  const targetParent = parentOrigin === "null" ? "*" : parentOrigin;
  let lastResult = null;
  let player,
    stopped = false,
    visible = false,
    played = false,
    seq = 0,
    serial = 0,
    execution = 0,
    metadata = null,
    blocked = false;
  const post = (extra = {}) => {
    if (stopped) return;
    try {
      const url = new URL(player?.getVideoUrl() || "https://www.youtube.com/");
      const id = ["youtube.com", "www.youtube.com"].includes(url.hostname)
        ? url.searchParams.get("v")
        : null;
      const duration = player?.getDuration() || 0,
        position = player?.getCurrentTime() || 0,
        state = player?.getPlayerState() ?? -1;
      const fraction = player?.getVideoLoadedFraction() || 0;
      parent.postMessage(
        {
          type: "movieparty-youtube",
          lease,
          seq: ++seq,
          videoId: id,
          player: {
            position_ms: Math.max(0, Math.round(position * 1000)),
            duration_ms: Math.max(0, Math.round(duration * 1000)),
            buffer_ms: Math.max(0, Math.round((duration * fraction - position) * 1000)),
            ready_state: played && (state === 1 || state === 2) ? 4 : 2,
            paused: state !== 1,
            ended: state === 0,
            seeking: state === 3,
          },
          playable: played && (state === 1 || state === 2) && visible,
          metadata,
          result: lastResult,
          ...extra,
        },
        targetParent,
      );
    } catch {
      /* API not ready yet; the bounded startup timer reports failure. */
    }
  };
  const dispose = () => {
    if (stopped) return;
    stopped = true;
    clearInterval(timer);
    clearTimeout(startup);
    execution++;
    try {
      player?.pauseVideo();
      player?.destroy();
    } catch {
      /* already gone */
    }
  };
  window.addEventListener("beforeunload", dispose);
  const wait = () => new Promise((resolve) => setTimeout(resolve, 80));
  window.addEventListener("message", async (event) => {
    const command = event.data;
    if (
      event.source !== parent ||
      event.origin !== parentOrigin ||
      command?.lease !== lease ||
      stopped
    )
      return;
    if (command.type === "dispose") {
      dispose();
      return;
    }
    if (command.type === "visible") {
      const wasVisible = visible;
      visible = command.visible === true;
      if (wasVisible && !visible) {
        execution++;
        try {
          player?.pauseVideo();
        } catch {
          /* API still loading */
        }
      }
      return;
    }
    if (
      command.type !== "command" ||
      !Number.isSafeInteger(command.serial) ||
      command.serial <= serial ||
      !player
    )
      return;
    if (
      !["PLAY", "PAUSE", "SEEK"].includes(command.action) ||
      !Number.isSafeInteger(command.target_ms) ||
      command.target_ms < 0
    )
      return;
    serial = command.serial;
    const own = ++execution;
    blocked = false;
    let outcome = "REJECTED";
    try {
      if (command.action === "PLAY" && Date.now() > command.expires_at_ms) {
        post({ result: { serial: command.serial, outcome: "TIMED_OUT" } });
        return;
      }
      if (command.action === "PLAY" && !visible) throw new Error("not visible");
      const startPosition = player.getCurrentTime();
      if (command.action === "PAUSE") player.pauseVideo();
      else if (command.action === "SEEK") {
        player.pauseVideo();
        player.seekTo(command.target_ms / 1000, true);
      } else {
        if (Math.abs(startPosition - command.target_ms / 1000) > 0.25)
          player.seekTo(command.target_ms / 1000, true);
        player.playVideo();
      }
      let stable = 0;
      for (let n = 0; n < 30 && !stopped && own === execution; n++) {
        await wait();
        if (blocked) {
          outcome = "GESTURE_REQUIRED";
          break;
        }
        const state = player.getPlayerState(),
          position = player.getCurrentTime();
        const close = Math.abs(position * 1000 - command.target_ms) <= 1500;
        if (command.action === "SEEK" && state === 1) player.pauseVideo();
        if (
          close &&
          (command.action === "PLAY"
            ? state === 1 && position > command.target_ms / 1000 + 0.05
            : state === 2 || state === 0)
        ) {
          if (++stable >= 3) {
            outcome = "APPLIED";
            break;
          }
        } else {
          stable = 0;
        }
      }
    } catch {
      outcome = blocked ? "GESTURE_REQUIRED" : "REJECTED";
    }
    if (own === execution) {
      lastResult = { serial: command.serial, outcome };
      post();
    }
  });
  const timer = setInterval(() => post(), 300);
  const startup = setTimeout(() => post({ error: 1001 }), 15000);
  window.onYouTubeIframeAPIReady = () => {
    if (stopped) return;
    player = new window.YT.Player("player", {
      width: "100%",
      height: "100%",
      videoId: video,
      playerVars: {
        autoplay: 0,
        controls: 1,
        playsinline: 1,
        origin: location.origin,
        widget_referrer: "https://app.movieparty.desktop",
      },
      events: {
        onReady: () => {
          clearTimeout(startup);
          player.cueVideoById(video);
          post();
        },
        onStateChange: (event) => {
          if (event.data === 1) played = true;
          post();
        },
        onError: (event) => post({ error: event.data }),
        onAutoplayBlocked: () => {
          blocked = true;
          post({ error: 1000 });
        },
      },
    });
  };
  const script = document.createElement("script");
  script.src = "https://www.youtube.com/iframe_api";
  script.onerror = () => post({ error: 1001 });
  document.head.appendChild(script);
  fetch(
    `https://www.youtube.com/oembed?url=${encodeURIComponent(`https://www.youtube.com/watch?v=${video}`)}&format=json`,
    { credentials: "omit", referrerPolicy: "strict-origin-when-cross-origin" },
  )
    .then((response) => {
      if (!response.ok) throw new Error("metadata unavailable");
      return response.json();
    })
    .then((value) => {
      metadata = { title: value.title, channel: value.author_name, thumbnail: value.thumbnail_url };
      post();
    })
    .catch(() => {
      /* Actual selected title remains available from room metadata. */
    });
})();
