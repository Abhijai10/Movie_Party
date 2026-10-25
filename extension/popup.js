// Popup: the only place the companion becomes active, so every read follows a
// deliberate click by the person who owns the browser.

const panels = {
  needsPairing: document.getElementById("needs-pairing"),
  ready: document.getElementById("ready"),
  active: document.getElementById("active"),
  error: document.getElementById("error"),
};

const CAPABILITY_LABELS = {
  identify_tab: "Recognise the tab you chose",
  report_content: "Report the selected title",
  report_playback: "Report playback position",
  request_navigation: "Open a title you approve",
  control_playback: "Play, pause or seek",
};

async function ask(type, extra = {}) {
  return chrome.runtime.sendMessage({ type, ...extra });
}

function show(phase, payload = {}) {
  for (const panel of Object.values(panels)) panel.hidden = true;
  if (phase === "OFF" || phase === "NEEDS_PAIRING") panels.needsPairing.hidden = false;
  else if (phase === "READY") panels.ready.hidden = false;
  else if (phase === "ACTIVE") panels.active.hidden = false;
  else panels.error.hidden = false;

  if (phase === "ERROR") {
    document.getElementById("error-note").textContent =
      payload.guidance ?? "The companion could not continue.";
  }
  if (phase === "ACTIVE") {
    const granted = new Set(payload.granted ?? []);
    const list = document.getElementById("capabilities");
    list.replaceChildren(
      ...Object.entries(CAPABILITY_LABELS).map(([name, label]) => {
        const item = document.createElement("li");
        const on = granted.has(name);
        if (!on) item.className = "off";
        const text = document.createElement("span");
        text.textContent = label;
        const state = document.createElement("span");
        state.textContent = on ? "allowed" : "off";
        item.append(text, state);
        return item;
      }),
    );
    document.getElementById("active-note").textContent = `Using: ${payload.host ?? "provider tab"}`;
  }
}

async function refresh() {
  const status = await ask("STATUS");
  if (!status?.ok) {
    show("ERROR", { guidance: "The companion could not report its state. Reopen Movie Party." });
    return;
  }
  if (status.phase === "READY" || status.phase === "ACTIVE") {
    const begun = await ask("BEGIN");
    show(begun?.phase ?? status.phase, begun ?? {});
    return;
  }
  show(status.phase, status);
}

document.getElementById("pair").addEventListener("click", async () => {
  const code = document.getElementById("code").value.trim().toUpperCase();
  const reply = await ask("PAIR", { code });
  show(reply?.ok ? "READY" : "ERROR", reply ?? {});
});

document.getElementById("use-tab").addEventListener("click", async () => {
  const reply = await ask("USE_ACTIVE_TAB");
  show(
    reply?.ok ? "ACTIVE" : "ERROR",
    reply?.ok ? { granted: reply.granted, host: reply.host } : reply,
  );
});

document.getElementById("again").addEventListener("click", async () => {
  await ask("BACK");
  refresh();
});

document.getElementById("retry").addEventListener("click", async () => {
  await ask("BACK");
  refresh();
});

document.getElementById("disconnect").addEventListener("click", async () => {
  await ask("DISCONNECT");
  show("OFF");
});

refresh();
