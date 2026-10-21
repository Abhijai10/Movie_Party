use super::*;
use crate::providers::content::ChooserSnapshot;

impl AppRuntime {
    pub fn confirmed_provider_url(
        &self,
        provider: &str,
        key: &str,
        requested: &str,
    ) -> Result<String, String> {
        self.lock()
            .provider_chooser
            .confirmed_url(provider, key, requested)
    }

    fn with_chooser_session<T>(
        &self,
        provider: &str,
        operation: impl FnOnce(
            &mut crate::providers::chrome::ManagedChromeSession,
            u64,
        ) -> Result<T, String>,
    ) -> Result<T, String> {
        if crate::providers::sync::provider_id_from_str(provider).is_none() {
            return Err("Unsupported provider.".into());
        }
        let (mut session, generation) = {
            let mut state = self.lock();
            if !Self::is_host_role(&state) {
                return Err("Only the host chooses content.".into());
            }
            if state.provider.provider_id.as_deref() != Some(provider) {
                return Err("Open this provider's managed browser first.".into());
            }
            let generation = state.chrome_generation;
            let session = state
                .chrome_session
                .take()
                .ok_or("The managed browser is closed or busy. Refresh, or reopen it.")?;
            (session, generation)
        };
        let result = if session.is_alive() {
            operation(&mut session, generation)
        } else {
            Err("The managed browser closed. Reopen it and select your content again.".into())
        };
        let mut stale = Some(session);
        let current = {
            let mut state = self.lock();
            let current = chrome_session_may_be_reinserted(&state, generation);
            if current {
                state.chrome_session = stale.take();
            }
            current
        };
        drop(stale);
        if !current {
            return Err("The browser session changed. Refresh the chooser.".into());
        }
        result
    }

    pub fn refresh_provider_content(&self, provider: &str) -> Result<ChooserSnapshot, String> {
        self.with_chooser_session(provider, |session, generation| {
            let tabs = session.content_tabs(provider).map_err(|e| e.to_string())?;
            let mut state = self.lock();
            if state.chrome_generation != generation {
                return Err("The browser changed. Refresh the chooser.".into());
            }
            state.provider_chooser.reconcile(tabs);
            if state.credentials.is_none() {
                session.pin_content_target(state.provider_chooser.selected.as_ref());
            }
            state.provider.readiness = if state.provider_chooser.content_ready {
                crate::providers::sync::ProviderReadiness::PlaybackReady
            } else {
                crate::providers::sync::ProviderReadiness::Ready
            };
            state.provider.url = state
                .provider_chooser
                .selected
                .as_ref()
                .and_then(|t| t.identity.as_ref())
                .map(|i| i.allowed_url.clone());
            Ok(state.provider_chooser.clone())
        })
    }

    pub fn confirm_provider_content(
        &self,
        provider: &str,
        target: &str,
        key: &str,
    ) -> Result<ChooserSnapshot, String> {
        let in_room = self.lock().credentials.is_some();
        self.refresh_provider_content(provider)?;
        self.with_chooser_session(provider, |session, generation| {
            // A second observation closes the preview/confirm race.
            let tabs = session.content_tabs(provider).map_err(|e| e.to_string())?;
            let mut state = self.lock();
            if state.chrome_generation != generation {
                return Err("The browser changed. Refresh the chooser.".into());
            }
            state.provider_chooser.reconcile(tabs);
            let mut confirmed = state.provider_chooser.clone();
            confirmed.confirm(target, key)?;
            // Drop the runtime lock before activation; it is another CDP trip.
            drop(state);
            session
                .connect_target(target)
                .map_err(|e| e.to_string())?
                .bring_to_front()
                .map_err(|e| e.to_string())?;
            let mut state = self.lock();
            if state.chrome_generation != generation {
                return Err("The browser changed. Refresh the chooser.".into());
            }
            state.provider_chooser = confirmed;
            session.pin_content_target(state.provider_chooser.selected.as_ref());
            state.provider.url = state
                .provider_chooser
                .selected
                .as_ref()
                .and_then(|t| t.identity.as_ref())
                .map(|i| i.allowed_url.clone());
            state.provider.readiness = if state.provider_chooser.content_ready {
                crate::providers::sync::ProviderReadiness::PlaybackReady
            } else {
                crate::providers::sync::ProviderReadiness::Ready
            };
            let chosen = state.provider_chooser.selected.clone();
            let snapshot = state.provider_chooser.clone();
            drop(state);
            if in_room {
                self.publish_provider_tab(chosen.ok_or("Choose content before publishing")?)?;
            }
            Ok(snapshot)
        })
    }

    pub fn browse_provider_content(
        &self,
        provider: &str,
        input: &str,
        direct: bool,
    ) -> Result<ChooserSnapshot, String> {
        if self.lock().credentials.is_some() {
            return Err("Leave the current party before browsing for different content.".into());
        }
        let id = crate::providers::sync::provider_id_from_str(provider)
            .ok_or("Unsupported provider.")?;
        let url = if direct {
            crate::providers::content::destination(provider, input)?
        } else {
            crate::providers::sync::provider_search_url(id, input)
                .filter(|_| input.len() <= 300)
                .ok_or("Enter a title to search (up to 300 characters).")?
        };
        self.with_chooser_session(provider, |session, generation| {
            let target = session.page_target().map_err(|e| e.to_string())?;
            let mut page = session.connect_target(&target.id).map_err(|e| e.to_string())?;
            page.navigate(&url).map_err(|e| e.to_string())?;
            page.bring_to_front().map_err(|e| e.to_string())?;
            // A Page.navigate acknowledgement alone is insufficient. Observe
            // the loaded destination; do not confirm metadata from the old page.
            let started = std::time::Instant::now();
            loop {
                let value = page.evaluate("location.href").map_err(|e| e.to_string())?;
                let actual = value["result"]["value"].as_str().unwrap_or_default();
                let matches = if direct {
                    crate::providers::content::destination(provider, actual).ok().as_deref() == Some(url.as_str())
                } else { actual == url || actual.strip_suffix('/') == Some(url.as_str()) };
                if matches { break; }
                if started.elapsed() >= std::time::Duration::from_secs(5) { return Err("The provider did not open that destination. Check Chrome for a sign-in or navigation error, then retry.".into()); }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            session.pin_content_target(None);
            let mut state = self.lock();
            if state.chrome_generation != generation { return Err("The browser changed. Refresh the chooser.".into()); }
            state.provider_chooser = ChooserSnapshot::default();
            state.provider.url = None;
            state.provider.readiness = crate::providers::sync::ProviderReadiness::Navigating;
            Ok(state.provider_chooser.clone())
        })
    }

    pub fn validate_confirmed_provider_content(
        &self,
        provider: &str,
        key: &str,
    ) -> Result<(), String> {
        let chooser = self.refresh_provider_content(provider)?;
        let verified = chooser.selected.as_ref().is_some_and(|tab| {
            tab.identity.as_ref().is_some_and(|identity| {
                tab.evidence.as_ref().is_some_and(|evidence| {
                    evidence.valid_for(&identity.provider_id, &identity.content_id)
                })
            })
        });
        if !verified || !chooser.content_ready || chooser.confirmed_key.as_deref() != Some(key) {
            return Err("Preview and confirm the selected content, then wait for its video to be ready in Chrome.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guest_cannot_select_host_content() {
        let runtime = AppRuntime::new();
        runtime.lock().local_participant.role = "Guest".into();
        assert!(runtime
            .refresh_provider_content("youtube")
            .unwrap_err()
            .contains("host"));
    }
    #[test]
    fn missing_or_wrong_browser_cannot_reuse_a_confirmation() {
        let runtime = AppRuntime::new();
        assert!(runtime
            .validate_confirmed_provider_content("youtube", "old-key")
            .is_err());
        runtime.lock().provider.provider_id = Some("youtube".into());
        runtime.lock().provider_chooser.confirmed_key = Some("old-key".into());
        runtime.lock().provider_chooser.content_ready = true;
        assert!(runtime
            .validate_confirmed_provider_content("youtube", "old-key")
            .is_err());
        assert!(runtime.refresh_provider_content("netflix").is_err());
    }
    #[test]
    fn browser_ready_is_insufficient_for_room_creation() {
        let runtime = AppRuntime::new();
        runtime.update_provider_readiness(crate::providers::sync::ProviderReadiness::Ready);
        assert!(runtime.validate_provider_ready_for_room().is_err());
    }
    #[test]
    fn javascript_false_reaches_existing_strict_pause_recovery() {
        let runtime = AppRuntime::new();
        runtime.lock().room_state = RoomState::Playing;
        let error = crate::providers::sync::verify_action_result(
            &serde_json::json!({"result":{"value":false}}),
        )
        .unwrap_err();
        let mapped = crate::providers::sync::command_error_to_runtime_error(&error);
        AppRuntime::record_provider_command_failure(&runtime.inner, mapped);
        let snapshot = runtime.snapshot();
        assert!(snapshot.sync.strict_sync_paused);
        assert!(snapshot.error.is_some());
    }
}
