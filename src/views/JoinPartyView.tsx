import { Button } from "../components/Button";
import { Card } from "../components/Card";
import { CinematicBackdrop } from "../components/CinematicBackdrop";
import { ErrorState } from "../components/ErrorState";
import { GlassPanel } from "../components/GlassPanel";
import { TextField } from "../components/TextField";
import type { AppSnapshot } from "../backend/appRuntime";
import { useState } from "react";

type JoinPartyViewProps = {
  snapshot: AppSnapshot;
  isJoining: boolean;
  error: string | null;
  onBack: () => void;
  onJoin: (inviteCode: string) => Promise<boolean>;
};

export function JoinPartyView({ snapshot, isJoining, error, onBack, onJoin }: JoinPartyViewProps) {
  const [inviteCode, setInviteCode] = useState("");
  const invitePreview = inviteCode.trim();
  const hasRoomContext = snapshot.room.roomId != null;

  return (
    <main className="app-shell centered-shell flow-shell">
      <CinematicBackdrop />
      <GlassPanel className="setup-panel flow-panel" labelledBy="join-title">
        <Button variant="ghost" onClick={onBack}>
          Back
        </Button>
        <p className="brand">Join Party</p>
        <h1 id="join-title">Join Move Party</h1>
        <p className="panel-copy">Paste your friend&apos;s invite link or enter the code manually.</p>
        <ErrorState message={error} />
        <TextField
          label="Invite link or code"
          value={inviteCode}
          disabled={isJoining}
          onChange={(event) => {
            setInviteCode(event.target.value);
          }}
          placeholder="moveparty://join/..."
          helperText="Your text stays here if the connection fails."
        />
        <Card className={error ? "invite-preview invalid" : "invite-preview"}>
          <strong>Invite preview</strong>
          <span>{invitePreview.length > 0 ? invitePreview : "Waiting for invite code"}</span>
        </Card>
        <Card className="qr-placeholder">
          <strong>QR scanning</strong>
          <span>Future backend integration. Manual invite entry is available now.</span>
        </Card>
        {hasRoomContext ? (
          <ErrorState message="Existing room context detected from the app runtime." tone="info" />
        ) : null}
        <Button
          variant="primary"
          isLoading={isJoining}
          onClick={() => {
            void onJoin(inviteCode);
          }}
        >
          Join Party
        </Button>
      </GlassPanel>
    </main>
  );
}
