//! Redacted debug projections for collaboration presentation models.

use super::*;

impl std::fmt::Debug for CollabPanelScreen {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("Unavailable"),
            Self::SignInRequired => formatter.write_str("SignInRequired"),
            Self::SignInUnavailable => formatter.write_str("SignInUnavailable"),
            Self::Home => formatter.write_str("Home"),
            Self::Create => formatter.write_str("Create"),
            Self::Join { discovered, .. } => formatter
                .debug_struct("Join")
                .field("target", &"[REDACTED]")
                .field("discovered", discovered)
                .finish(),
            Self::Progress { message } => formatter
                .debug_struct("Progress")
                .field("message", message)
                .finish(),
            Self::ConfirmOwner(model) => formatter
                .debug_tuple("ConfirmOwner")
                .field(&**model)
                .finish(),
            Self::Session {
                session_name,
                role_label,
                invite,
                connection,
                share_endpoint,
                participants,
                pending,
                admission_request,
            } => formatter
                .debug_struct("Session")
                .field("session_name", session_name)
                .field("role_label", role_label)
                .field("invite", invite)
                .field("connection", connection)
                .field("share_endpoint", share_endpoint)
                .field("participants", participants)
                .field("pending", pending)
                .field("admission_request", admission_request)
                .finish(),
        }
    }
}

impl std::fmt::Debug for CollabPanelActionModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let action = match self.action {
            CollabUiAction::OpenCreate => "OpenCreate",
            CollabUiAction::Start => "Start",
            CollabUiAction::StartLan => "StartLan",
            CollabUiAction::SetRelayRegion {
                region: op_editor_core::CollabRelayRegion::China,
            } => "SetRelayRegion(China)",
            CollabUiAction::SetRelayRegion {
                region: op_editor_core::CollabRelayRegion::Global,
            } => "SetRelayRegion(Global)",
            CollabUiAction::OpenJoin => "OpenJoin",
            CollabUiAction::BeginDiscovery => "BeginDiscovery",
            CollabUiAction::JoinDiscovered { .. } => "JoinDiscovered([REDACTED])",
            CollabUiAction::JoinAddress { .. } => "JoinAddress([REDACTED])",
            CollabUiAction::Cancel => "Cancel",
            CollabUiAction::Retry => "Retry",
            CollabUiAction::Leave => "Leave",
            CollabUiAction::DiscardPending => "DiscardPending",
            CollabUiAction::ReapplyDiscarded => "ReapplyDiscarded",
            CollabUiAction::SaveAsFork => "SaveAsFork",
            CollabUiAction::ApproveAdmissionEditor { .. } => "ApproveAdmissionEditor([REDACTED])",
            CollabUiAction::ApproveAdmissionViewer { .. } => "ApproveAdmissionViewer([REDACTED])",
            CollabUiAction::RejectAdmission { .. } => "RejectAdmission([REDACTED])",
            CollabUiAction::ConfirmOwnerIdentity { .. } => "ConfirmOwnerIdentity([REDACTED])",
            CollabUiAction::RejectOwnerIdentity { .. } => "RejectOwnerIdentity([REDACTED])",
        };
        formatter
            .debug_struct("CollabPanelActionModel")
            .field("action", &action)
            .field("label", &self.label)
            .field("primary", &self.primary)
            .finish()
    }
}
