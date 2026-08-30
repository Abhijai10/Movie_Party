use serde_json::json;

use super::{chrome::CdpCommand, generic::GenericProviderAdapter};

pub const PROVIDER_ID: &str = "netflix";

#[derive(Debug, Clone, Copy, Default)]
pub struct NetflixAdapter {
    generic: GenericProviderAdapter,
}

impl NetflixAdapter {
    pub fn login_required(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "Boolean(location.hostname.includes('netflix.com') && (location.pathname.includes('/login') || document.querySelector('[data-uia=\"login-submit-button\"], input[name=\"userLoginId\"]')))",
        )
    }

    pub fn detect_media(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "Boolean(location.hostname.includes('netflix.com') && document.querySelector('video'))",
        )
    }

    pub fn media_identity(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "(() => ({ title: document.title || '', content_id: location.pathname.split('/').filter(Boolean).pop() || null, provider: 'netflix' }))()",
        )
    }

    pub fn play(&self, id: u64) -> CdpCommand {
        self.generic.play(id)
    }

    pub fn pause(&self, id: u64) -> CdpCommand {
        self.generic.pause(id)
    }

    pub fn seek(&self, id: u64, seconds: f64) -> CdpCommand {
        self.generic.seek(id, seconds)
    }

    pub fn get_position(&self, id: u64) -> CdpCommand {
        self.generic.get_position(id)
    }

    pub fn get_buffer_state(&self, id: u64) -> CdpCommand {
        self.generic.get_buffer_state(id)
    }
}

fn runtime_command(id: u64, expression: &str) -> CdpCommand {
    CdpCommand {
        id,
        method: "Runtime.evaluate",
        params: json!({
            "expression": expression,
            "awaitPromise": true,
            "returnByValue": true
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression(command: &CdpCommand) -> &str {
        command.params["expression"].as_str().expect("expression")
    }

    #[test]
    fn netflix_adapter_has_login_media_and_control_paths() {
        let adapter = NetflixAdapter::default();

        assert!(expression(&adapter.login_required(1)).contains("login"));
        assert!(expression(&adapter.detect_media(2)).contains("netflix.com"));
        assert!(expression(&adapter.media_identity(3)).contains("content_id"));
        assert!(expression(&adapter.play(4)).contains(".play()"));
        assert!(expression(&adapter.pause(5)).contains(".pause()"));
        assert!(expression(&adapter.seek(6, 12.0)).contains("currentTime = 12"));
        assert!(expression(&adapter.get_position(7)).contains("currentTime"));
        assert!(expression(&adapter.get_buffer_state(8)).contains("buffered.end"));
    }
}
