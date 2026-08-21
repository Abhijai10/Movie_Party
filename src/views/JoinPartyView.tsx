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
      <GlassPanel className="setup-panel invitation-stage" labelledBy="join-title">
        <Button className="back-button" variant="ghost" onClick={onBack}>
          ← Back
        </Button>
        <div className="invitation-heading"><p className="brand">Private invitation</p><h1 id="join-title">Your seat is waiting.</h1><p className="panel-copy">Enter the invite from your friend and join their cinema when the room is ready.</p></div>
        <ErrorState message={error} onRetry={() => { if (invitePreview) { void onJoin(invitePreview); } }} />
        <div className="join-layout">
          <div className="join-primary">
            <TextField label="Invite link or code" value={inviteCode} disabled={isJoining} onChange={(event) => setInviteCode(event.target.value)} placeholder="moveparty://join/{roomId}" helperText="Paste an invite link, or open one directly from Move Party." />
            <div className="invite-connection-stage">
              <div className="invite-seat" aria-hidden="true"><span /><span /><span /></div>
              <div className={error ? "invite-preview invalid" : "invite-preview"}>
                <span>Invitation</span>
                <strong>{invitePreview.length > 0 ? invitePreview : "Waiting for your invite"}</strong>
                <small>{isJoining ? "Opening a secure path to the room..." : error ? "The invite needs another look" : "Your place is reserved when the connection opens"}</small>
              </div>
            </div>
            <Button variant="primary" isLoading={isJoining} onClick={() => { void onJoin(inviteCode); }}>Join party</Button>
          </div>
          <Card className="qr-placeholder join-qr-secondary"><div className="qr-mark" aria-hidden="true"><span /><span /><span /></div><strong>Invite QR</strong><span>QR joining is not available yet.</span></Card>
        </div>
        {hasRoomContext ? (
          <ErrorState message="Existing room context detected from the app runtime." tone="info" />
        ) : null}
      </GlassPanel>
    </main>
  );
}
