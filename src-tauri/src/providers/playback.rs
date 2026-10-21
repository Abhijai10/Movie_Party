//! Observed provider operations. No URLs, titles, account data or raw errors in reports.
use super::selection::SelectionContext;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Action {
    Play,
    Pause,
    Seek,
}
impl Action {
    pub fn from_commit(kind: &str) -> Option<Self> {
        match kind {
            "PLAY" => Some(Self::Play),
            "PAUSE" => Some(Self::Pause),
            "SEEK" => Some(Self::Seek),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Outcome {
    Applied,
    GestureRequired,
    Rejected,
    TimedOut,
    Buffering,
    Ended,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub context: SelectionContext,
    pub operation_id: String,
    pub action: Action,
    pub outcome: Outcome,
    pub position_ms: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Phase {
    Verifying,
    Applied,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub context: SelectionContext,
    pub operation_id: String,
    pub sequence: u64,
    pub action: Action,
    pub target_ms: u64,
    pub resume: bool,
    pub phase: Phase,
    pub host: Option<Report>,
    pub guest: Option<Report>,
}
impl Operation {
    pub fn follows(&self, old: &Self) -> bool {
        if self.sequence > old.sequence {
            return true;
        }
        if self.sequence < old.sequence
            || self.operation_id != old.operation_id
            || self.context != old.context
            || self.action != old.action
            || self.target_ms != old.target_ms
            || self.resume != old.resume
        {
            return false;
        }
        if old.phase != Phase::Verifying {
            return self == old;
        }
        old.host
            .as_ref()
            .is_none_or(|r| self.host.as_ref() == Some(r))
            && old
                .guest
                .as_ref()
                .is_none_or(|r| self.guest.as_ref() == Some(r))
    }
    pub fn accepts(&self, report: &Report) -> bool {
        self.phase == Phase::Verifying
            && self.context == report.context
            && self.operation_id == report.operation_id
            && self.action == report.action
            && (report.outcome != Outcome::Applied
                || report.position_ms.abs_diff(self.target_ms) <= 1500)
    }
    pub fn record(&mut self, report: Report, host: bool, solo: bool) -> bool {
        if !self.accepts(&report) {
            return false;
        }
        let slot = if host {
            &mut self.host
        } else {
            &mut self.guest
        };
        if let Some(old) = slot {
            return old == &report;
        }
        let failed = report.outcome != Outcome::Applied;
        *slot = Some(report);
        if failed {
            self.phase = Phase::Failed;
        } else if self.host.is_some() && (self.guest.is_some() || solo) {
            self.phase = Phase::Applied;
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub buffer_ms: u64,
    pub ready_state: u8,
    pub paused: bool,
    pub ended: bool,
    pub seeking: bool,
}
impl Observation {
    pub fn ready(&self) -> bool {
        !self.ended
            && !self.seeking
            && self.ready_state >= 3
            && self.duration_ms > 0
            && (self.buffer_ms >= 3000
                || self.buffer_ms >= self.duration_ms.saturating_sub(self.position_ms))
    }
    pub fn verifies(&self, action: Action, target: u64) -> bool {
        !self.seeking
            && self.position_ms.abs_diff(target) <= 1500
            && match action {
                Action::Play => self.ready() && !self.paused,
                Action::Pause => self.paused,
                Action::Seek => self.ready() && self.paused,
            }
    }
}

/// Uses only the unique visible HTML video. Bounded promise/seek waits; never
/// supplies CDP userGesture or automates authentication. Follow-up observation
/// and identity validation are required by the caller before accepting success.
pub fn action_expression(action: Action, target_ms: u64) -> String {
    let body = match action {
        Action::Play => "if (Math.abs(v.currentTime-target)>0.25) { v.pause(); v.currentTime=target; } await Promise.race([v.play(),new Promise((_,reject)=>setTimeout(()=>reject(new Error('timeout')),2000))]);",
        Action::Pause => "v.pause();",
        Action::Seek => "v.pause(); v.currentTime=target;",
    };
    format!(
        r#"(async()=>{{
      const vs=[...document.querySelectorAll('video')].filter(v=>{{const r=v.getBoundingClientRect();return r.width>0&&r.height>0;}});
      if(vs.length!==1)return 'REJECTED'; const v=vs[0],target={target_ms}/1000;
      try{{ {body}
        for(let n=0;n<20 && v.seeking;n++)await new Promise(r=>setTimeout(r,100));
        await new Promise(r=>setTimeout(r,150));
        return v.error ? 'REJECTED' : 'APPLIED';
      }}catch(e){{v.pause();return e?.name==='NotAllowedError'?'GESTURE_REQUIRED':'REJECTED';}}
    }})()"#
    )
}

/// Closed-schema local diagnostics: arbitrary input text cannot enter a log.
pub fn diagnostic(action: Action, outcome: Outcome, revision: u64) {
    tracing::info!(event = "provider_playback", ?action, ?outcome, revision);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn op() -> Operation {
        Operation {
            context: SelectionContext {
                state_epoch: uuid::Uuid::now_v7().to_string(),
                host_sequence: 2,
                content_revision: 2,
            },
            operation_id: uuid::Uuid::now_v7().to_string(),
            sequence: 1,
            action: Action::Play,
            target_ms: 1000,
            resume: true,
            phase: Phase::Verifying,
            host: None,
            guest: None,
        }
    }
    fn report(o: &Operation) -> Report {
        Report {
            context: o.context.clone(),
            operation_id: o.operation_id.clone(),
            action: o.action,
            outcome: Outcome::Applied,
            position_ms: 1000,
        }
    }
    #[test]
    fn both_observed_results_required_duplicate_and_stale_safe() {
        let mut o = op();
        let r = report(&o);
        assert!(o.record(r.clone(), true, false));
        assert_eq!(o.phase, Phase::Verifying);
        assert!(o.record(r.clone(), true, false));
        let mut stale = r.clone();
        stale.context.content_revision -= 1;
        assert!(!o.record(stale, false, false));
        assert!(o.record(r, false, false));
        assert_eq!(o.phase, Phase::Applied);
    }
    #[test]
    fn rejected_seek_timeout_gesture_and_mismatch_never_succeed() {
        for outcome in [
            Outcome::Rejected,
            Outcome::TimedOut,
            Outcome::GestureRequired,
            Outcome::Buffering,
            Outcome::Ended,
        ] {
            let mut o = op();
            o.action = Action::Seek;
            let mut r = report(&o);
            r.outcome = outcome;
            assert!(o.record(r, false, false));
            assert_eq!(o.phase, Phase::Failed);
        }
        let mut o = op();
        let mut r = report(&o);
        r.position_ms = 9000;
        assert!(!o.record(r, true, false));
    }
    #[test]
    fn false_ready_buffering_and_eof_are_distinct() {
        let mut p = Observation {
            position_ms: 1000,
            duration_ms: 10000,
            buffer_ms: 5000,
            ready_state: 4,
            paused: false,
            ended: false,
            seeking: false,
        };
        assert!(p.ready());
        assert!(p.verifies(Action::Play, 1000));
        p.paused = true;
        assert!(!p.verifies(Action::Play, 1000));
        assert!(p.verifies(Action::Pause, 1000));
        p.buffer_ms = 0;
        assert!(!p.ready());
        p.ended = true;
        assert!(!p.ready());
        assert!(p.ended);
    }
    #[test]
    fn terminal_decisions_cannot_rewind_and_reports_cannot_smuggle_secrets() {
        let mut current = op();
        let mut applied = current.clone();
        applied.host = Some(report(&applied));
        applied.guest = Some(report(&applied));
        applied.phase = Phase::Applied;
        assert!(applied.follows(&current));
        assert!(!current.follows(&applied));
        current.phase = Phase::Failed;
        assert!(!current.follows(&applied));
        let mut data = serde_json::to_value(report(&applied)).unwrap();
        data["cookie"] = serde_json::json!("https://provider.invalid/private?token=secret");
        assert!(serde_json::from_value::<Report>(data).is_err());
        let clean = serde_json::to_string(&applied).unwrap();
        for forbidden in ["https:", "cookie", "password", "canonical_url", "title"] {
            assert!(!clean.contains(forbidden));
        }
    }
}
