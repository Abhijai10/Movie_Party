pub const PRELOAD_SAFETY_MULTIPLIER: f32 = 1.4;
pub const PREPARATION_MARGIN_MS: i64 = 15 * 60 * 1_000;

pub mod preload;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleStatus {
    Planned,
    WaitingForHost,
    WaitingForGuest,
    Transferring,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieSchedule {
    pub schedule_id: String,
    pub room_id: String,
    pub media_id: String,
    pub scheduled_start_utc_ms: i64,
    pub planned_preload_utc_ms: i64,
    pub guest_device_id: String,
    pub status: ScheduleStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreloadInputs {
    pub remaining_bytes: u64,
    pub conservative_goodput_bps: u64,
    pub scheduled_start_utc_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPlan {
    pub preload_soon_utc_ms: i64,
    pub preload_start_utc_ms: i64,
    pub party_soon_utc_ms: i64,
    pub party_imminent_utc_ms: i64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SchedulingError {
    #[error("MP-SCHEDULE-001 goodput must be positive")]
    MissingGoodput,
}

pub fn calculate_preload_start(inputs: PreloadInputs) -> Result<i64, SchedulingError> {
    if inputs.conservative_goodput_bps == 0 {
        return Err(SchedulingError::MissingGoodput);
    }

    let transfer_time_ms = ((inputs.remaining_bytes as f64 * 8.0 * 1_000.0)
        / inputs.conservative_goodput_bps as f64) as i64;
    let safe_transfer_time_ms = (transfer_time_ms as f32 * PRELOAD_SAFETY_MULTIPLIER) as i64;

    Ok(inputs
        .scheduled_start_utc_ms
        .saturating_sub(safe_transfer_time_ms)
        .saturating_sub(PREPARATION_MARGIN_MS))
}

pub fn notification_plan(schedule: &MovieSchedule) -> NotificationPlan {
    NotificationPlan {
        preload_soon_utc_ms: schedule.planned_preload_utc_ms - 30 * 60 * 1_000,
        preload_start_utc_ms: schedule.planned_preload_utc_ms,
        party_soon_utc_ms: schedule.scheduled_start_utc_ms - 15 * 60 * 1_000,
        party_imminent_utc_ms: schedule.scheduled_start_utc_ms - 2 * 60 * 1_000,
    }
}

pub fn peer_availability_status(host_online: bool, guest_online: bool) -> ScheduleStatus {
    match (host_online, guest_online) {
        (true, true) => ScheduleStatus::Transferring,
        (false, _) => ScheduleStatus::WaitingForHost,
        (_, false) => ScheduleStatus::WaitingForGuest,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        calculate_preload_start, notification_plan, peer_availability_status, MovieSchedule,
        PreloadInputs, ScheduleStatus,
    };

    #[test]
    fn calculates_preload_start_with_safety_and_margin() {
        let start = calculate_preload_start(PreloadInputs {
            remaining_bytes: 1_000_000_000,
            conservative_goodput_bps: 8_000_000,
            scheduled_start_utc_ms: 10_000_000,
        })
        .expect("preload");

        assert_eq!(start, 7_700_000);
    }

    #[test]
    fn builds_notification_plan() {
        let schedule = MovieSchedule {
            schedule_id: "schedule".to_string(),
            room_id: "room".to_string(),
            media_id: "media".to_string(),
            scheduled_start_utc_ms: 10_000_000,
            planned_preload_utc_ms: 7_000_000,
            guest_device_id: "guest".to_string(),
            status: ScheduleStatus::Planned,
        };

        let plan = notification_plan(&schedule);
        assert_eq!(plan.preload_start_utc_ms, 7_000_000);
        assert_eq!(plan.party_imminent_utc_ms, 9_880_000);
    }

    #[test]
    fn tracks_offline_peer_state() {
        assert_eq!(
            peer_availability_status(true, false),
            ScheduleStatus::WaitingForGuest,
        );
        assert_eq!(
            peer_availability_status(true, true),
            ScheduleStatus::Transferring,
        );
    }
}
