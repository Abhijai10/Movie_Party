pub const GHOST_SHORTCUT: &str = "Ctrl/Cmd+Shift+M";
pub const PRIVACY_SHORTCUT: &str = "Ctrl/Cmd+Shift+P";
pub const GHOST_CONFIRMATION_MS: u64 = 800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivacyState {
    pub ghost_mode: bool,
    pub privacy_mode: bool,
    pub camera_enabled: bool,
    pub microphone_enabled: bool,
}

impl PrivacyState {
    pub fn new(camera_enabled: bool, microphone_enabled: bool) -> Self {
        Self {
            ghost_mode: false,
            privacy_mode: false,
            camera_enabled,
            microphone_enabled,
        }
    }

    pub fn toggle_ghost(&mut self) {
        if self.privacy_mode {
            return;
        }

        self.ghost_mode = !self.ghost_mode;
    }

    pub fn toggle_privacy(&mut self) -> PrivacyTransition {
        if self.privacy_mode {
            self.privacy_mode = false;
            self.ghost_mode = false;

            return PrivacyTransition::ExitedPrivacyMediaStillDisabled;
        }

        self.privacy_mode = true;
        self.ghost_mode = true;
        self.camera_enabled = false;
        self.microphone_enabled = false;
        PrivacyTransition::EnteredPrivacy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyTransition {
    EnteredPrivacy,
    ExitedPrivacyMediaStillDisabled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghost_mode_hides_ui_without_changing_media_state() {
        let mut state = PrivacyState::new(true, true);

        state.toggle_ghost();

        assert!(state.ghost_mode);
        assert!(state.camera_enabled);
        assert!(state.microphone_enabled);
    }

    #[test]
    fn privacy_mode_enables_ghost_and_disables_camera_and_mic() {
        let mut state = PrivacyState::new(true, true);

        assert_eq!(state.toggle_privacy(), PrivacyTransition::EnteredPrivacy);

        assert!(state.ghost_mode);
        assert!(state.privacy_mode);
        assert!(!state.camera_enabled);
        assert!(!state.microphone_enabled);
    }

    #[test]
    fn leaving_privacy_restores_ui_but_not_camera_or_microphone() {
        let mut state = PrivacyState::new(true, true);

        state.toggle_privacy();

        assert_eq!(
            state.toggle_privacy(),
            PrivacyTransition::ExitedPrivacyMediaStillDisabled
        );

        assert!(!state.ghost_mode);
        assert!(!state.privacy_mode);
        assert!(!state.camera_enabled);
        assert!(!state.microphone_enabled);
    }

    #[test]
    fn ghost_shortcut_is_ignored_while_privacy_mode_is_active() {
        let mut state = PrivacyState::new(true, true);

        state.toggle_privacy();
        state.toggle_ghost();

        assert!(state.ghost_mode);
        assert!(state.privacy_mode);
    }
}
