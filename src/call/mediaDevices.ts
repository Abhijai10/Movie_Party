export type CallDeviceInventory = {
  cameras: MediaDeviceInfo[];
  microphones: MediaDeviceInfo[];
  available: boolean;
  errorCode?: "MP-CALL-001" | "MP-CALL-002";
};

export async function enumerateCallDevices(): Promise<CallDeviceInventory> {
  const runtimeNavigator = navigator as { mediaDevices?: MediaDevices };
  const mediaDevices = runtimeNavigator.mediaDevices;

  if (!mediaDevices) {
    return {
      cameras: [],
      microphones: [],
      available: false,
      errorCode: "MP-CALL-001",
    };
  }

  try {
    const devices = await mediaDevices.enumerateDevices();

    return {
      cameras: devices.filter((device) => device.kind === "videoinput"),
      microphones: devices.filter((device) => device.kind === "audioinput"),
      available: true,
    };
  } catch {
    return {
      cameras: [],
      microphones: [],
      available: false,
      errorCode: "MP-CALL-002",
    };
  }
}
