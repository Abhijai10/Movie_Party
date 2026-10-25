//! Batch 10 runtime: the facts the access UI needs, and nothing more.
//!
//! There is deliberately no HTTP client in this file. The capability matrix says
//! no operation qualifies for automation, so the runtime's job is to state what
//! is true locally — this machine's name, the tailnet suffix, the user's role,
//! whether a share is still visible — and to hand the user the one console page
//! that can act.

use super::*;
use crate::app_runtime::friendship::normalised_peer_key;
use crate::network::tailscale::{self, readiness_from_status, TailnetRole, TailscaleReadiness};
use crate::tailscale_access::{
    build_share_plan, revocation_state, CapabilityRow, DestructiveAction, RevocationState,
    SharePlan, ShareRoleVerdict, CAPABILITY_MATRIX,
};

use std::collections::HashSet;

/// Facts the access surface may hold between calls. Only peer keys that were
/// *observed* as shared into this tailnet: without that memory, a node's absence
/// would be indistinguishable from never having been there.
#[derive(Debug, Default)]
pub struct TailscaleAccessState {
    shared_in_seen: HashSet<String>,
}

impl TailscaleAccessState {
    pub(crate) fn note(&mut self, peer_key: &str) {
        self.shared_in_seen.insert(peer_key.to_string());
    }

    pub(crate) fn was_seen(&self, peer_key: &str) -> bool {
        self.shared_in_seen.contains(peer_key)
    }
}

/// The matrix as the UI receives it, plus the one derived fact that matters:
/// whether *any* operation is automatable today (it is not, and the flag exists
/// so a UI cannot invent a button the matrix does not support).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessMatrixView {
    pub rows: Vec<CapabilityRow>,
    pub any_automatable: bool,
    pub shared_with_external_user_note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePlanView {
    pub plan: SharePlan,
    pub role_allowed: bool,
    pub role_unknown: bool,
    /// The sentence to read before opening the console.
    pub confirmation: String,
    /// What Movie Party will *not* do, stated where the button is.
    pub not_performed: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevocationView {
    pub peer_key: String,
    pub state: RevocationState,
    pub copy: &'static str,
    pub console_url: Option<&'static str>,
}

const NOT_PERFORMED: &str =
    "Movie Party cannot create or revoke a Tailscale share: the documented API has no sharing endpoint or scope. It opens Tailscale's own page and reports what it can see.";

impl AppRuntime {
    /// The documented capability matrix, verbatim from the module that cites it.
    pub fn tailscale_access_matrix(&self) -> AccessMatrixView {
        AccessMatrixView {
            rows: CAPABILITY_MATRIX.to_vec(),
            any_automatable: CAPABILITY_MATRIX
                .iter()
                .any(|row| row.verdict == crate::tailscale_access::AutomationVerdict::Automatable),
            shared_with_external_user_note:
                "Sharing is one machine to one person. The recipient must sign in and accept with their own Tailscale account, and must be an Owner, Admin or IT admin of their own tailnet to do so.",
        }
    }

    /// Builds the plan for sharing *this* machine. Reads local status only,
    /// refuses to guess an unreported role, and never mutates anything.
    pub async fn tailscale_share_plan(
        &self,
        recipient: &str,
        lifetime_days: u32,
    ) -> Result<SharePlanView, String> {
        let status = tailscale::detect_status().await.map_err(|_| {
            access_error(crate::tailscale_access::AccessFailure::LocalStatusUnavailable)
        })?;
        let readiness: TailscaleReadiness = readiness_from_status(status.clone());
        if !readiness.is_usable() && !tailscale::dev_loopback_enabled() {
            return Err(readiness.stable_error().unwrap_or_else(|| {
                access_error(crate::tailscale_access::AccessFailure::LocalStatusUnavailable)
            }));
        }
        self.note_observed_shares(&status);
        let role = status
            .tailnet
            .as_ref()
            .map(|identity| identity.role)
            .unwrap_or(TailnetRole::Unknown);
        let plan = build_share_plan(
            status.device_name.as_deref().unwrap_or_default(),
            status
                .tailnet
                .as_ref()
                .and_then(|identity| identity.magic_dns_suffix.as_deref()),
            role,
            recipient,
            lifetime_days,
        )
        .map_err(|error| error.to_string())?;
        if plan.role == ShareRoleVerdict::NotAllowed {
            return Err(access_error(
                crate::tailscale_access::AccessFailure::InsufficientPrivileges,
            ));
        }
        let confirmation = plan.confirmation();
        Ok(SharePlanView {
            role_allowed: plan.role == ShareRoleVerdict::Allowed,
            role_unknown: plan.role == ShareRoleVerdict::Unknown,
            plan,
            confirmation,
            not_performed: NOT_PERFORMED,
        })
    }

    /// Whether a share is still visible from here. Claims "no longer visible"
    /// only when this device previously saw the node as shared in; otherwise the
    /// answer is `Unknown` and the console is the only authority.
    pub async fn tailscale_revocation_state(
        &self,
        peer_key: &str,
    ) -> Result<RevocationView, String> {
        let peer_key = normalised_peer_key(peer_key.trim());
        let status = tailscale::detect_status().await.map_err(|_| {
            access_error(crate::tailscale_access::AccessFailure::LocalStatusUnavailable)
        })?;
        let visible = status
            .peers
            .iter()
            .any(|peer| normalised_peer_key(&peer.dns_name) == peer_key);
        self.note_observed_shares(&status);
        let previously = self.lock().tailscale_access.was_seen(&peer_key);
        let state = revocation_state(previously, visible);
        Ok(RevocationView {
            peer_key,
            state,
            copy: state.copy(),
            console_url: DestructiveAction::RevokeTailscaleDeviceShare.console_url(),
        })
    }

    /// Opens one of the two admin pages. Callers pass an action id so the UI
    /// cannot open an arbitrary URL through this path.
    pub fn tailscale_console_url(&self, action: &str) -> Result<&'static str, String> {
        let action = DestructiveAction::from_id(action).ok_or_else(|| {
            "MP-ACCESS-020 that is not one of the three removal actions".to_string()
        })?;
        action.console_url().ok_or_else(|| {
            "MP-ACCESS-021 removing a Movie Party friend is a local action and has no console page"
                .to_string()
        })
    }

    /// Records peers that arrived from another tailnet. Called on every status
    /// read that the access surface performs.
    fn note_observed_shares(&self, status: &tailscale::TailscaleStatus) {
        let shared: Vec<String> = status
            .peers
            .iter()
            .filter(|peer| {
                peer.shared_in || tailscale::peer_is_same_tailnet(status, peer) == Some(false)
            })
            .map(|peer| normalised_peer_key(&peer.dns_name))
            .collect();
        if shared.is_empty() {
            return;
        }
        let mut state = self.lock();
        for peer_key in shared {
            state.tailscale_access.note(&peer_key);
        }
    }
}

fn access_error(failure: crate::tailscale_access::AccessFailure) -> String {
    format!("{} {}", failure.code(), failure.guidance())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> (AppRuntime, std::path::PathBuf) {
        let runtime = AppRuntime::new();
        let dir = std::env::temp_dir().join(format!("mp-access-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        runtime.init_db_at_path(&dir.join("mp.db"));
        (runtime, dir)
    }

    #[test]
    fn the_matrix_view_never_offers_automation() {
        let (runtime, dir) = runtime();
        let view = runtime.tailscale_access_matrix();
        assert!(!view.any_automatable, "nothing may be automatable today");
        assert_eq!(view.rows.len(), CAPABILITY_MATRIX.len());
        assert!(view
            .shared_with_external_user_note
            .contains("sign in and accept"));
        let json = serde_json::to_string(&view).expect("serializes");
        assert!(json.contains("\"CREATE_DEVICE_SHARE\""));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn only_the_two_admin_actions_have_console_urls() {
        let (runtime, dir) = runtime();
        assert_eq!(
            runtime
                .tailscale_console_url("REVOKE_TAILSCALE_DEVICE_SHARE")
                .expect("url"),
            "https://login.tailscale.com/admin/machines"
        );
        assert_eq!(
            runtime
                .tailscale_console_url("REMOVE_TAILNET_MEMBER")
                .expect("url"),
            "https://login.tailscale.com/admin/users"
        );
        assert!(runtime
            .tailscale_console_url("REMOVE_MOVIE_PARTY_FRIEND")
            .unwrap_err()
            .starts_with("MP-ACCESS-021"));
        assert!(runtime
            .tailscale_console_url("OPEN_EVERYTHING")
            .unwrap_err()
            .starts_with("MP-ACCESS-020"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn observed_shares_are_remembered_so_absence_means_something() {
        let (runtime, dir) = runtime();
        let peer = "priya-mac.tail99aa00.ts.net.";
        assert!(!runtime.lock().tailscale_access.was_seen(peer));
        // A node from another tailnet, as the status would report it.
        let status = tailscale::parse_status_json(
            format!(
                r#"{{"BackendState":"Running","CurrentTailnet":{{"StableID":"Tn1","MagicDNSSuffix":"tailc930b7.ts.net"}},
                    "Self":{{"DNSName":"abhijai-mac.tailc930b7.ts.net.","TailscaleIPs":["100.64.0.10"],"CapMap":{{"https://tailscale.com/cap/is-owner":[[]]}}}},
                    "Peer":{{"k":{{"DNSName":"{peer}","TailscaleIPs":["100.64.0.42"],"Online":true,"ShareeNode":true}}}}}}"#
            )
            .as_bytes(),
        )
        .expect("status");
        runtime.note_observed_shares(&status);
        assert!(runtime.lock().tailscale_access.was_seen(peer));
        // Same-tailnet peers are not "shared in" and must not be recorded.
        assert!(!runtime
            .lock()
            .tailscale_access
            .was_seen("othersame.tailc930b7.ts.net."));
        std::fs::remove_dir_all(dir).ok();
    }
}
