//! Batch 10 — what Movie Party may and may not automate on Tailscale's control
//! plane, and why.
//!
//! Every row in the matrix below cites the page it came from. The rule the
//! module enforces is deliberately blunt: **an operation is automatable only if
//! the published documentation names both an endpoint and a scope narrow enough
//! for it.** As of 2026-10-26 that set is empty, so `automation_allowed` is
//! always false and the wizard keeps Batch 9's guided console path as the real
//! implementation rather than shipping a button that cannot work.
//!
//! What the documentation actually says (quoted in [`CAPABILITY_MATRIX`]):
//!
//! * Sharing a machine requires an Owner, Admin or IT admin, and the *recipient*
//!   must also be an Owner/Admin/IT admin of a tailnet to accept — there is no
//!   zero-click path for either side, by design.
//! * The published trust-credential scopes include device invites
//!   (`devices_invites`) but **no scope for machine sharing and none for user
//!   invites**, even though user-invite endpoints exist.
//! * Device invites and auth keys exist and are narrow-ish, but using them would
//!   enrol the other person's machine under authority the host owns, which
//!   Batch 10 forbids outright.
//!
//! The three removal-ish actions are kept apart on purpose: removing a Movie
//! Party friend, revoking a Tailscale device share, and removing a tailnet member
//! are different acts with different blast radii, and the UI must never offer
//! one as if it were another.

use serde::{Deserialize, Serialize};

use crate::network::tailscale::TailnetRole;

/// Something a user might hope Movie Party can do for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccessOperation {
    /// Invite a person to become a member of the tailnet.
    CreateUserInvite,
    ListUserInvites,
    RevokeUserInvite,
    /// Share one machine with a person outside the tailnet.
    CreateDeviceShare,
    ListDeviceShares,
    RevokeDeviceShare,
    /// Remove a person from the tailnet entirely.
    RemoveTailnetMember,
    /// Mint an auth key that a device can enrol with.
    CreateAuthKey,
}

impl AccessOperation {
    pub fn label(self) -> &'static str {
        match self {
            AccessOperation::CreateUserInvite => "Invite a user to the tailnet",
            AccessOperation::ListUserInvites => "List open user invitations",
            AccessOperation::RevokeUserInvite => "Revoke a user invitation",
            AccessOperation::CreateDeviceShare => "Share this machine with one person",
            AccessOperation::ListDeviceShares => "List machine shares",
            AccessOperation::RevokeDeviceShare => "Revoke a machine share",
            AccessOperation::RemoveTailnetMember => "Remove a tailnet member",
            AccessOperation::CreateAuthKey => "Create an auth key",
        }
    }
}

/// How far Movie Party may go, based strictly on what is documented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutomationVerdict {
    /// Endpoint and a purpose-specific scope are both documented. Only such an
    /// operation may be automated, and none currently qualifies.
    Automatable,
    /// Endpoints exist, but the published scope list names no scope for it (or
    /// only a tailnet-wide one), so the least privilege it can run with is
    /// broader than the action justifies.
    DocumentedButUnscoped,
    /// Only the admin console performs it. Movie Party guides and observes.
    ConsoleOnly,
    /// Possible with the API, refused by this product's rules.
    ForbiddenByPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRow {
    pub operation: AccessOperation,
    pub verdict: AutomationVerdict,
    /// The least role the vendor documentation requires, when it states one.
    pub required_role: &'static str,
    /// A scope string, only when the published scope table names one for it.
    pub scope: Option<&'static str>,
    /// The sentence this row is based on, quoted from the vendor page.
    pub evidence_quote: &'static str,
    pub evidence_url: &'static str,
    /// What the wizard does instead.
    pub product_response: &'static str,
}

pub const SHARING_URL: &str = "https://tailscale.com/docs/features/sharing";
pub const ROLES_URL: &str = "https://tailscale.com/docs/reference/user-roles";
pub const SCOPES_URL: &str = "https://tailscale.com/docs/reference/trust-credentials";
pub const API_URL: &str = "https://tailscale.com/docs/reference/tailscale-api";
pub const CHANGELOG_URL: &str = "https://tailscale.com/changelog";

/// Quoted from the vendor pages above. Kept as data so a stale claim is a failing
/// test rather than a sentence somebody has to remember to re-read.
pub const CAPABILITY_MATRIX: [CapabilityRow; 8] = [
    CapabilityRow {
        operation: AccessOperation::CreateDeviceShare,
        verdict: AutomationVerdict::ConsoleOnly,
        required_role: "Owner, Admin or IT admin of the sharing tailnet",
        scope: None,
        evidence_quote: "A shared machine can receive incoming connections but cannot start connections.",
        evidence_url: SHARING_URL,
        product_response:
            "The wizard opens the Tailscale admin page and states exactly which machine to share; Movie Party never calls an API for this.",
    },
    CapabilityRow {
        operation: AccessOperation::ListDeviceShares,
        verdict: AutomationVerdict::ConsoleOnly,
        required_role: "Owner, Admin or IT admin",
        scope: None,
        evidence_quote:
            "The published trust-credential scope list contains devices, users, policy, keys and logs scopes — and no scope for machine sharing.",
        evidence_url: SCOPES_URL,
        product_response:
            "Revocation is confirmed from what this device can observe (the shared node leaving the tailnet view), and otherwise the user is sent to the console.",
    },
    CapabilityRow {
        operation: AccessOperation::RevokeDeviceShare,
        verdict: AutomationVerdict::ConsoleOnly,
        required_role: "Owner, Admin or IT admin",
        scope: None,
        evidence_quote: "You can revoke a share or share invite from the Machines page of the admin console.",
        evidence_url: SHARING_URL,
        product_response:
            "Movie Party never claims it revoked a share. It reports the observed state and links to the page that can.",
    },
    CapabilityRow {
        operation: AccessOperation::CreateUserInvite,
        verdict: AutomationVerdict::DocumentedButUnscoped,
        required_role: "Owner or Admin",
        scope: None,
        evidence_quote:
            "User-invite endpoints exist (list, create, get), but the published scope table names no scope for them.",
        evidence_url: CHANGELOG_URL,
        product_response:
            "Not enabled: an operation whose least privilege is unnamed cannot be shown to be narrow. Invitations stay a console step.",
    },
    CapabilityRow {
        operation: AccessOperation::ListUserInvites,
        verdict: AutomationVerdict::DocumentedButUnscoped,
        required_role: "Owner or Admin",
        scope: None,
        evidence_quote: "The spec names no scope for reading user invites.",
        evidence_url: SCOPES_URL,
        product_response: "Read from the Tailscale admin console instead.",
    },
    CapabilityRow {
        operation: AccessOperation::RevokeUserInvite,
        verdict: AutomationVerdict::DocumentedButUnscoped,
        required_role: "Owner or Admin",
        scope: None,
        evidence_quote: "Inviting and removing users is an Owner/Admin action in the documented role table.",
        evidence_url: ROLES_URL,
        product_response: "Console step, with the role requirement stated before the user starts.",
    },
    CapabilityRow {
        operation: AccessOperation::RemoveTailnetMember,
        verdict: AutomationVerdict::DocumentedButUnscoped,
        required_role: "Owner, Admin or IT admin",
        scope: None,
        evidence_quote: "They can perform any action in the admin console, including inviting or removing users.",
        evidence_url: ROLES_URL,
        product_response:
            "Kept separate from removing a friend and from revoking a share. Never performed by Movie Party.",
    },
    CapabilityRow {
        operation: AccessOperation::CreateAuthKey,
        verdict: AutomationVerdict::ForbiddenByPolicy,
        required_role: "Owner, Admin or IT admin",
        scope: Some("auth_keys"),
        evidence_quote: "Auth keys authenticate devices and can be reusable; they grant tailnet access in the holder's name.",
        evidence_url: "https://tailscale.com/docs/features/access-control/auth-keys",
        product_response:
            "Never created or embedded: enrolling someone else's machine with a key the host owns is exactly what Batch 10 prohibits.",
    },
];

pub fn matrix_row(operation: AccessOperation) -> &'static CapabilityRow {
    CAPABILITY_MATRIX
        .iter()
        .find(|row| row.operation == operation)
        .expect("every operation has a matrix row")
}

/// The whole point of the matrix: nothing may be automated unless a narrow,
/// documented scope exists for it. Today that set is empty, and this function is
/// what the runtime asks before it would ever offer an API call.
pub fn automation_allowed(operation: AccessOperation) -> bool {
    matrix_row(operation).verdict == AutomationVerdict::Automatable
}

/// Roles the vendor documentation names for sharing a machine.
///
/// `TailnetRole::Unknown` is reported as "cannot tell", never as "not allowed",
/// for the same reason Batch 9 refuses to turn an unreported role into
/// `LimitedUser`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShareRoleVerdict {
    Allowed,
    NotAllowed,
    Unknown,
}

pub fn share_role_verdict(role: TailnetRole) -> ShareRoleVerdict {
    match role {
        TailnetRole::Owner | TailnetRole::Admin => ShareRoleVerdict::Allowed,
        TailnetRole::LimitedUser => ShareRoleVerdict::NotAllowed,
        TailnetRole::Unknown => ShareRoleVerdict::Unknown,
    }
}

/// What the user is about to share. Deliberately a *plan*, not a request: it is
/// built from local facts, validated, and then handed to the console path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePlan {
    /// The machine Movie Party would share, as this device names itself.
    pub device_name: String,
    pub tailnet_suffix: Option<String>,
    /// Exactly one recipient account. Never a group, never a tag.
    pub recipient: String,
    /// Days until the unused link expires. The vendor's own ceiling is 30.
    pub lifetime_days: u32,
    pub role: ShareRoleVerdict,
    pub console_url: String,
}

/// Vendor-documented ceiling for an unused share link.
pub const MAX_SHARE_LIFETIME_DAYS: u32 = 30;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SharePlanError {
    #[error("MP-ACCESS-001 this device has no tailnet name, so Movie Party cannot say which machine to share")]
    NoDeviceName,
    #[error("MP-ACCESS-002 a share is for one person; add exactly one recipient account")]
    RecipientNotSingle,
    #[error("MP-ACCESS-003 that does not look like an account email or login")]
    RecipientInvalid,
    #[error("MP-ACCESS-004 share links expire after at most {MAX_SHARE_LIFETIME_DAYS} days")]
    LifetimeTooLong,
    #[error("MP-ACCESS-005 a share link has to live for at least a day to be usable")]
    LifetimeTooShort,
    #[error("MP-ACCESS-006 there is no tailnet suffix to address the console page with")]
    NoTailnetSuffix,
}

/// Builds the plan for sharing **this** machine with exactly one person.
///
/// Requirements 8 and 9 in one function: the device name comes from this
/// device's own Tailscale identity (never a peer, never the tailnet), and the
/// recipient/lifetime are bounded. Anything that could widen the share is an
/// error, not a warning.
pub fn build_share_plan(
    device_name: &str,
    tailnet_suffix: Option<&str>,
    role: TailnetRole,
    recipient: &str,
    lifetime_days: u32,
) -> Result<SharePlan, SharePlanError> {
    let device = device_name.trim().trim_end_matches('.');
    if device.is_empty() || !device.contains('.') {
        return Err(SharePlanError::NoDeviceName);
    }
    let recipient = recipient.trim();
    if recipient.is_empty() || recipient.split_whitespace().count() != 1 {
        return Err(SharePlanError::RecipientNotSingle);
    }
    // A mail address or a login name; both appear in Tailscale's own share flow.
    if !recipient.contains('@') || recipient.len() < 5 || recipient.contains(',') {
        return Err(SharePlanError::RecipientInvalid);
    }
    if lifetime_days == 0 {
        return Err(SharePlanError::LifetimeTooShort);
    }
    if lifetime_days > MAX_SHARE_LIFETIME_DAYS {
        return Err(SharePlanError::LifetimeTooLong);
    }
    let suffix = tailnet_suffix
        .map(|value| value.trim().trim_end_matches('.').to_string())
        .filter(|value| !value.is_empty())
        .ok_or(SharePlanError::NoTailnetSuffix)?;
    Ok(SharePlan {
        device_name: device.to_string(),
        tailnet_suffix: Some(suffix.clone()),
        recipient: recipient.to_string(),
        lifetime_days,
        role: share_role_verdict(role),
        // The Machines page is where the vendor documentation says shares are
        // created and revoked; it is never fetched by the app.
        console_url: "https://login.tailscale.com/admin/machines".to_string(),
    })
}

/// What a share plan is *not*: the tailnet, a second device, a tag, a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShareScope {
    ThisMachineOnly,
}

impl SharePlan {
    pub fn scope(&self) -> ShareScope {
        ShareScope::ThisMachineOnly
    }

    /// The sentence the wizard shows before the user opens the console.
    pub fn confirmation(&self) -> String {
        format!(
            "Share only this machine ({}) with {} for {} days. Tailscale's recipient must sign in and accept; nothing is shared until they do, and you can revoke it from the Machines page at any time.",
            self.device_name, self.recipient, self.lifetime_days
        )
    }
}

/// Whether a share's revocation can be confirmed from this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RevocationState {
    /// It was visible as a shared-in node before and is absent now.
    NoLongerVisible,
    /// It is still listed as a shared node: not revoked.
    StillShared,
    /// Never observed as shared here, so absence proves nothing.
    Unknown,
}

/// Local, observable evidence about a share. A node shared in from another
/// tailnet appears in this device's status; after revocation it stops appearing.
/// Absence is only meaningful if it was seen before — which is why the first
/// field exists.
pub fn revocation_state(previously_observed: bool, currently_visible: bool) -> RevocationState {
    match (previously_observed, currently_visible) {
        (true, false) => RevocationState::NoLongerVisible,
        (_, true) => RevocationState::StillShared,
        (false, false) => RevocationState::Unknown,
    }
}

impl RevocationState {
    /// Requirement 14 in one sentence: confirm what can be confirmed, and point
    /// at the console for the rest.
    pub fn copy(self) -> &'static str {
        match self {
            RevocationState::NoLongerVisible => {
                "The shared machine is no longer visible in this device's tailnet, which is consistent with a revoked share. Confirm it on Tailscale's Machines page."
            }
            RevocationState::StillShared => {
                "The shared machine is still visible in this device's tailnet. Revoke it on Tailscale's Machines page."
            }
            RevocationState::Unknown => {
                "Movie Party never saw this machine as a share from here, so it cannot confirm a revocation either way. Check Tailscale's Machines page."
            }
        }
    }
}

/// The three actions that must never be presented as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DestructiveAction {
    /// Local trust only. Reversible by inviting again (they must accept).
    RemoveMoviePartyFriend,
    /// Tailscale-level access to one machine. Owner/Admin/IT admin, explicitly
    /// not performed by Movie Party today.
    RevokeTailscaleDeviceShare,
    /// Tailnet membership. The broadest of the three.
    RemoveTailnetMember,
}

impl DestructiveAction {
    pub fn id(self) -> &'static str {
        match self {
            DestructiveAction::RemoveMoviePartyFriend => "REMOVE_MOVIE_PARTY_FRIEND",
            DestructiveAction::RevokeTailscaleDeviceShare => "REVOKE_TAILSCALE_DEVICE_SHARE",
            DestructiveAction::RemoveTailnetMember => "REMOVE_TAILNET_MEMBER",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "REMOVE_MOVIE_PARTY_FRIEND" => Some(DestructiveAction::RemoveMoviePartyFriend),
            "REVOKE_TAILSCALE_DEVICE_SHARE" => Some(DestructiveAction::RevokeTailscaleDeviceShare),
            "REMOVE_TAILNET_MEMBER" => Some(DestructiveAction::RemoveTailnetMember),
            _ => None,
        }
    }

    /// The admin page that performs it. Opened only on an explicit click; the
    /// app never fetches it.
    pub fn console_url(self) -> Option<&'static str> {
        match self {
            DestructiveAction::RemoveMoviePartyFriend => None,
            DestructiveAction::RevokeTailscaleDeviceShare => {
                Some("https://login.tailscale.com/admin/machines")
            }
            DestructiveAction::RemoveTailnetMember => {
                Some("https://login.tailscale.com/admin/users")
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DestructiveAction::RemoveMoviePartyFriend => "Remove Movie Party friend",
            DestructiveAction::RevokeTailscaleDeviceShare => "Revoke Tailscale device share",
            DestructiveAction::RemoveTailnetMember => "Remove tailnet member",
        }
    }

    /// What stops working, stated in the user's terms.
    pub fn effect(self) -> &'static str {
        match self {
            DestructiveAction::RemoveMoviePartyFriend => {
                "Movie Party stops trusting this identity. It does not change Tailscale access, and Tailscale connectivity keeps working."
            }
            DestructiveAction::RevokeTailscaleDeviceShare => {
                "That machine stops accepting connections from them. Other devices on your tailnet are unaffected."
            }
            DestructiveAction::RemoveTailnetMember => {
                "Their account leaves the tailnet, losing every device in it — not only this app."
            }
        }
    }

    /// Whether this action can be done without the Tailscale admin console.
    pub fn performed_by(self) -> &'static str {
        match self {
            DestructiveAction::RemoveMoviePartyFriend => "Movie Party, locally",
            DestructiveAction::RevokeTailscaleDeviceShare => "Tailscale admin console",
            DestructiveAction::RemoveTailnetMember => "Tailscale admin console",
        }
    }

    pub fn needs_console(self) -> bool {
        !matches!(self, DestructiveAction::RemoveMoviePartyFriend)
    }

    /// The single thing this action must never be mistaken for.
    pub fn is_not(self, other: DestructiveAction) -> bool {
        self != other
    }
}

/// Requirement 12, in the order the user can actually hit them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccessFailure {
    /// The signed-in Tailscale user is not Owner/Admin/IT admin.
    InsufficientPrivileges,
    /// The CLI did not report a role at all — do not guess in either direction.
    RoleUnknown,
    /// The tailnet's plan or policy does not offer sharing/invites.
    PlanOrPolicyUnsupported,
    /// A documented endpoint answered with an error (only reachable if an
    /// operation ever becomes automatable).
    ApiFailure,
    /// The invitation or consent window has passed.
    ExpiredConsent,
    /// The CLI could not be asked at all.
    LocalStatusUnavailable,
}

impl AccessFailure {
    pub fn code(self) -> &'static str {
        match self {
            AccessFailure::InsufficientPrivileges => "MP-ACCESS-010",
            AccessFailure::RoleUnknown => "MP-ACCESS-011",
            AccessFailure::PlanOrPolicyUnsupported => "MP-ACCESS-012",
            AccessFailure::ApiFailure => "MP-ACCESS-013",
            AccessFailure::ExpiredConsent => "MP-ACCESS-014",
            AccessFailure::LocalStatusUnavailable => "MP-ACCESS-015",
        }
    }

    pub fn guidance(self) -> &'static str {
        match self {
            AccessFailure::InsufficientPrivileges => {
                "Your Tailscale login is not an Owner, Admin or IT admin of this tailnet, so it cannot create shares or invitations. Ask whoever administers the tailnet, or use a tailnet you own."
            }
            AccessFailure::RoleUnknown => {
                "Tailscale did not report your role on this tailnet. Open the admin console to check, rather than assuming either way."
            }
            AccessFailure::PlanOrPolicyUnsupported => {
                "This tailnet's plan or policy does not offer that action. An administrator can check the plan and the tailnet settings."
            }
            AccessFailure::ApiFailure => {
                "The request reached Tailscale but was refused or failed. Nothing was changed; try again or use the admin console."
            }
            AccessFailure::ExpiredConsent => {
                "That invitation or consent has expired. Create a fresh one — an expired link cannot be revived."
            }
            AccessFailure::LocalStatusUnavailable => {
                "Tailscale's local status could not be read, so Movie Party cannot tell what to do next. Open Tailscale, then check again."
            }
        }
    }

    pub fn blocking(self) -> bool {
        true
    }

    /// Maps HTTP status codes from a future API call, kept here so the mapping is
    /// tested long before any call exists.
    pub fn from_http_status(status: u16) -> Self {
        match status {
            401 => AccessFailure::ApiFailure,
            403 => AccessFailure::InsufficientPrivileges,
            404 | 410 => AccessFailure::ExpiredConsent,
            422 => AccessFailure::PlanOrPolicyUnsupported,
            _ => AccessFailure::ApiFailure,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_covers_every_operation_exactly_once() {
        for operation in [
            AccessOperation::CreateUserInvite,
            AccessOperation::ListUserInvites,
            AccessOperation::RevokeUserInvite,
            AccessOperation::CreateDeviceShare,
            AccessOperation::ListDeviceShares,
            AccessOperation::RevokeDeviceShare,
            AccessOperation::RemoveTailnetMember,
            AccessOperation::CreateAuthKey,
        ] {
            let rows: Vec<&CapabilityRow> = CAPABILITY_MATRIX
                .iter()
                .filter(|row| row.operation == operation)
                .collect();
            assert_eq!(rows.len(), 1, "{operation:?} must appear once");
            assert!(!rows[0].evidence_quote.is_empty());
            assert!(rows[0].evidence_url.starts_with("https://"));
            assert!(!rows[0].product_response.is_empty());
        }
    }

    #[test]
    fn nothing_is_automatable_without_a_named_narrow_scope() {
        // The invariant that keeps a "Create Share" button from being added
        // without the documentation to back it.
        for row in CAPABILITY_MATRIX.iter() {
            if row.verdict == AutomationVerdict::Automatable {
                assert!(
                    row.scope.is_some(),
                    "{:?} claims to be automatable without naming a scope",
                    row.operation
                );
            }
        }
        for operation in [
            AccessOperation::CreateDeviceShare,
            AccessOperation::ListDeviceShares,
            AccessOperation::RevokeDeviceShare,
            AccessOperation::CreateUserInvite,
            AccessOperation::ListUserInvites,
            AccessOperation::RevokeUserInvite,
            AccessOperation::RemoveTailnetMember,
            AccessOperation::CreateAuthKey,
        ] {
            assert!(
                !automation_allowed(operation),
                "{operation:?} must stay manual today"
            );
        }
    }

    #[test]
    fn sharing_is_console_only_and_says_so_with_the_vendors_words() {
        let row = matrix_row(AccessOperation::CreateDeviceShare);
        assert_eq!(row.verdict, AutomationVerdict::ConsoleOnly);
        assert_eq!(row.scope, None);
        assert!(row.required_role.contains("Owner"));
        assert!(row.evidence_quote.contains("cannot start connections"));
    }

    #[test]
    fn auth_keys_are_refused_by_policy_even_though_a_scope_exists() {
        let row = matrix_row(AccessOperation::CreateAuthKey);
        assert_eq!(row.verdict, AutomationVerdict::ForbiddenByPolicy);
        assert_eq!(row.scope, Some("auth_keys"));
        assert!(!automation_allowed(AccessOperation::CreateAuthKey));
    }

    #[test]
    fn user_invites_are_unscoped_not_automatable() {
        for operation in [
            AccessOperation::CreateUserInvite,
            AccessOperation::ListUserInvites,
            AccessOperation::RevokeUserInvite,
        ] {
            let row = matrix_row(operation);
            assert_eq!(row.verdict, AutomationVerdict::DocumentedButUnscoped);
            assert_eq!(row.scope, None);
        }
    }

    #[test]
    fn share_roles_follow_the_vendor_table_and_never_guess() {
        assert_eq!(
            share_role_verdict(TailnetRole::Owner),
            ShareRoleVerdict::Allowed
        );
        assert_eq!(
            share_role_verdict(TailnetRole::Admin),
            ShareRoleVerdict::Allowed
        );
        assert_eq!(
            share_role_verdict(TailnetRole::LimitedUser),
            ShareRoleVerdict::NotAllowed
        );
        assert_eq!(
            share_role_verdict(TailnetRole::Unknown),
            ShareRoleVerdict::Unknown,
            "an unreported role must not be treated as a refusal"
        );
    }

    #[test]
    fn a_share_plan_names_this_machine_one_person_and_a_bounded_lifetime() {
        let plan = build_share_plan(
            "abhijai-mac.tailc930b7.ts.net.",
            Some("tailc930b7.ts.net"),
            TailnetRole::Owner,
            "priya@example.com",
            7,
        )
        .expect("plan");
        assert_eq!(plan.device_name, "abhijai-mac.tailc930b7.ts.net");
        assert_eq!(plan.recipient, "priya@example.com");
        assert_eq!(plan.lifetime_days, 7);
        assert_eq!(plan.role, ShareRoleVerdict::Allowed);
        assert_eq!(plan.scope(), ShareScope::ThisMachineOnly);
        assert_eq!(
            plan.console_url,
            "https://login.tailscale.com/admin/machines"
        );
        let confirmation = plan.confirmation();
        assert!(confirmation.contains("only this machine"));
        assert!(confirmation.contains("priya@example.com"));
        assert!(confirmation.contains("sign in and accept"));
    }

    #[test]
    fn a_plan_cannot_be_widened_into_a_tailnet_or_a_group() {
        // No device name.
        assert_eq!(
            build_share_plan("", Some("t.ts.net"), TailnetRole::Owner, "a@b.co", 7),
            Err(SharePlanError::NoDeviceName)
        );
        // Several recipients.
        assert_eq!(
            build_share_plan(
                "mac.t.ts.net",
                Some("t.ts.net"),
                TailnetRole::Owner,
                "a@b.co c@d.co",
                7
            ),
            Err(SharePlanError::RecipientNotSingle)
        );
        // Not an account.
        assert_eq!(
            build_share_plan(
                "mac.t.ts.net",
                Some("t.ts.net"),
                TailnetRole::Owner,
                "tag:prod",
                7
            ),
            Err(SharePlanError::RecipientInvalid)
        );
        // Longer than the vendor's ceiling.
        assert_eq!(
            build_share_plan(
                "mac.t.ts.net",
                Some("t.ts.net"),
                TailnetRole::Owner,
                "a@b.co",
                31
            ),
            Err(SharePlanError::LifetimeTooLong)
        );
        // Zero-length consent.
        assert_eq!(
            build_share_plan(
                "mac.t.ts.net",
                Some("t.ts.net"),
                TailnetRole::Owner,
                "a@b.co",
                0
            ),
            Err(SharePlanError::LifetimeTooShort)
        );
        // No tailnet suffix to address the console.
        assert_eq!(
            build_share_plan("mac.t.ts.net", None, TailnetRole::Owner, "a@b.co", 7),
            Err(SharePlanError::NoTailnetSuffix)
        );
        // The ceiling itself is allowed.
        assert!(build_share_plan(
            "mac.t.ts.net",
            Some("t.ts.net"),
            TailnetRole::Owner,
            "a@b.co",
            MAX_SHARE_LIFETIME_DAYS
        )
        .is_ok());
    }

    #[test]
    fn revocation_is_only_confirmed_when_the_node_was_seen_and_then_left() {
        assert_eq!(
            revocation_state(true, false),
            RevocationState::NoLongerVisible
        );
        assert_eq!(revocation_state(true, true), RevocationState::StillShared);
        assert_eq!(revocation_state(false, true), RevocationState::StillShared);
        assert_eq!(
            revocation_state(false, false),
            RevocationState::Unknown,
            "absence without a prior observation proves nothing"
        );
        assert!(RevocationState::NoLongerVisible
            .copy()
            .contains("Confirm it on Tailscale's Machines page"));
        assert!(RevocationState::StillShared.copy().contains("Revoke it"));
    }

    #[test]
    fn the_three_removals_are_distinct_actions_with_distinct_effects() {
        let actions = [
            DestructiveAction::RemoveMoviePartyFriend,
            DestructiveAction::RevokeTailscaleDeviceShare,
            DestructiveAction::RemoveTailnetMember,
        ];
        for (index, action) in actions.iter().enumerate() {
            for (other_index, other) in actions.iter().enumerate() {
                if index != other_index {
                    assert!(action.is_not(*other));
                    assert_ne!(action.label(), other.label());
                    assert_ne!(action.effect(), other.effect());
                }
            }
        }
        // Removing a friend must say plainly what it does not do.
        assert!(DestructiveAction::RemoveMoviePartyFriend
            .effect()
            .contains("does not change Tailscale access"));
        // Revoking a share must not read as removing the member.
        assert!(DestructiveAction::RevokeTailscaleDeviceShare
            .effect()
            .contains("Other devices on your tailnet are unaffected"));
        // Removing a member must own its blast radius.
        assert!(DestructiveAction::RemoveTailnetMember
            .effect()
            .contains("every device"));
        // Only the local one is performed by Movie Party.
        assert!(!DestructiveAction::RemoveMoviePartyFriend.needs_console());
        assert!(DestructiveAction::RevokeTailscaleDeviceShare.needs_console());
        assert!(DestructiveAction::RemoveTailnetMember.needs_console());
        assert_eq!(
            DestructiveAction::RevokeTailscaleDeviceShare.performed_by(),
            "Tailscale admin console"
        );
    }

    #[test]
    fn failures_carry_codes_guidance_and_the_status_mapping_is_honest() {
        for failure in [
            AccessFailure::InsufficientPrivileges,
            AccessFailure::RoleUnknown,
            AccessFailure::PlanOrPolicyUnsupported,
            AccessFailure::ApiFailure,
            AccessFailure::ExpiredConsent,
            AccessFailure::LocalStatusUnavailable,
        ] {
            assert!(failure.code().starts_with("MP-ACCESS-"));
            assert!(failure.guidance().len() > 40);
            assert!(failure.blocking());
        }
        assert_eq!(
            AccessFailure::from_http_status(403),
            AccessFailure::InsufficientPrivileges
        );
        assert_eq!(
            AccessFailure::from_http_status(404),
            AccessFailure::ExpiredConsent
        );
        assert_eq!(
            AccessFailure::from_http_status(410),
            AccessFailure::ExpiredConsent
        );
        assert_eq!(
            AccessFailure::from_http_status(422),
            AccessFailure::PlanOrPolicyUnsupported
        );
        assert_eq!(
            AccessFailure::from_http_status(500),
            AccessFailure::ApiFailure
        );
    }

    #[test]
    fn the_console_actions_round_trip_and_the_local_one_has_no_url() {
        assert_eq!(
            DestructiveAction::from_id("REVOKE_TAILSCALE_DEVICE_SHARE"),
            Some(DestructiveAction::RevokeTailscaleDeviceShare)
        );
        assert_eq!(
            DestructiveAction::from_id("remove_tailnet_member"),
            Some(DestructiveAction::RemoveTailnetMember)
        );
        assert_eq!(DestructiveAction::from_id("DELETE_EVERYTHING"), None);
        assert_eq!(
            DestructiveAction::RemoveMoviePartyFriend.console_url(),
            None
        );
        assert!(DestructiveAction::RevokeTailscaleDeviceShare
            .console_url()
            .expect("url")
            .ends_with("/admin/machines"));
        assert!(DestructiveAction::RemoveTailnetMember
            .console_url()
            .expect("url")
            .ends_with("/admin/users"));
    }

    #[test]
    fn every_operation_has_human_copy() {
        for operation in [
            AccessOperation::CreateUserInvite,
            AccessOperation::ListUserInvites,
            AccessOperation::RevokeUserInvite,
            AccessOperation::CreateDeviceShare,
            AccessOperation::ListDeviceShares,
            AccessOperation::RevokeDeviceShare,
            AccessOperation::RemoveTailnetMember,
            AccessOperation::CreateAuthKey,
        ] {
            assert!(!operation.label().is_empty());
        }
    }
}
