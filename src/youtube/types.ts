export type SelectionContext = {
  state_epoch: string;
  host_sequence: number;
  content_revision: number;
};
export type YoutubeCommand = {
  serial: number;
  context: SelectionContext;
  operation_id: string | null;
  action: "PLAY" | "PAUSE" | "SEEK";
  target_ms: number;
  generation: number;
  expires_at_ms: number;
};
export type YoutubeMetadata = { title: string; channel: string; thumbnail: string };
export type YoutubeSnapshot = {
  enabled: boolean;
  chosen: boolean;
  lease: string | null;
  command: YoutubeCommand | null;
  metadata: YoutubeMetadata | null;
  error: number | null;
};
export type Attachment = { url: string; origin: string; lease: string };
export type PlayerEvent = {
  type: "movieparty-youtube";
  lease: string;
  seq: number;
  videoId: string | null;
  player: {
    position_ms: number;
    duration_ms: number;
    buffer_ms: number;
    ready_state: number;
    paused: boolean;
    ended: boolean;
    seeking: boolean;
  };
  playable: boolean;
  metadata: YoutubeMetadata | null;
  error?: number;
  result?: {
    serial: number;
    outcome: "APPLIED" | "REJECTED" | "GESTURE_REQUIRED" | "TIMED_OUT";
  } | null;
};
export function contextKey(context: SelectionContext): string {
  return `${context.state_epoch}:${String(context.host_sequence)}:${String(context.content_revision)}`;
}
export function acceptsMessage(
  event: MessageEvent,
  frame: Window | null,
  origin: string,
  lease: string,
  lastSeq: number,
): event is MessageEvent<PlayerEvent> {
  const data: unknown = event.data;
  if (
    event.source !== frame ||
    event.origin !== origin ||
    typeof data !== "object" ||
    data === null
  )
    return false;
  const value = data as Partial<PlayerEvent>;
  return (
    value.type === "movieparty-youtube" &&
    value.lease === lease &&
    Number.isSafeInteger(value.seq) &&
    Number(value.seq) > lastSeq &&
    Boolean(value.player)
  );
}
export function youtubeError(code: number | null | undefined): string | null {
  switch (code) {
    case 101:
    case 150:
      return "The owner does not allow embedded playback. Try this video in managed Chrome.";
    case 153:
      return "YouTube could not identify this desktop player. Try managed Chrome.";
    case 100:
      return "This video is unavailable or private. Use your own authorized account in Chrome, or choose another video.";
    case 2:
      return "YouTube rejected this video ID. Choose a supported video URL.";
    case 5:
      return "This WebView could not play the video. Try managed Chrome.";
    case 1000:
      return "YouTube needs a click. Press play in the YouTube player once, then pause and retry Ready.";
    case 1001:
      return "The YouTube player did not load. Check your connection, retry, or use managed Chrome.";
    default:
      return null;
  }
}
