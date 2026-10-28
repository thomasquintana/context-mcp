//! Durable job transitions and stale-publication fencing.

use crate::ids::{AssetId, JobId, PrincipalId, RevisionId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Durable execution state. Failed/cancelled jobs cannot publish a revision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Admission committed; execution has not started.
    Accepted,
    /// Required processing is in progress.
    Running,
    /// Every required stage and publication succeeded.
    Succeeded,
    /// Processing failed; no new revision was published.
    Failed,
    /// Cancellation committed; late output must not publish.
    Cancelled,
}

/// An invalid state transition, without payloads or implementation details.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("job state transition is not allowed")]
pub struct InvalidTransition;

impl JobState {
    /// Whether the state is immutable terminal history.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }

    /// Validate a transition. Repetition is idempotent; terminal outcomes cannot change.
    pub fn transition(self, next: Self) -> Result<Self, InvalidTransition> {
        match (self, next) {
            (current, next) if current == next => Ok(current),
            (Self::Accepted, Self::Running | Self::Failed | Self::Cancelled)
            | (Self::Running, Self::Succeeded | Self::Failed | Self::Cancelled) => Ok(next),
            _ => Err(InvalidTransition),
        }
    }
}

/// Admission-time generation identifying work permitted to publish.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationFence {
    /// Logical target asset.
    pub asset: AssetId,
    /// Generation captured atomically when work was admitted.
    pub generation: u64,
}

impl PublicationFence {
    /// Check under the publication transaction's resource lock.
    ///
    /// Purge must advance the generation and close admission before deleting
    /// content. Physical writes need the same coordination and cleanup discipline.
    pub fn permits(self, asset: AssetId, generation: u64, purge_in_progress: bool) -> bool {
        self.asset == asset && self.generation == generation && !purge_in_progress
    }
}

/// Durable ingestion/reprocessing job, separate from HTLK's execution records.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IngestionJob {
    /// Application job identity.
    pub id: JobId,
    /// Host-authenticated initiating identity.
    pub principal: PrincipalId,
    /// Immutable revision being prepared.
    pub revision: RevisionId,
    /// Target generation and asset.
    pub fence: PublicationFence,
    /// Current application lifecycle.
    pub state: JobState,
    /// Registered operator workflow identity/digest.
    pub workflow: String,
    /// HTLK run identity once admission has been reconciled.
    pub runtime_run: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_results_cannot_be_overwritten_and_unstarted_jobs_cannot_succeed() {
        assert_eq!(
            JobState::Accepted.transition(JobState::Succeeded),
            Err(InvalidTransition)
        );
        for state in [JobState::Succeeded, JobState::Failed, JobState::Cancelled] {
            assert!(state.is_terminal());
            assert_eq!(state.transition(state), Ok(state));
            assert_eq!(state.transition(JobState::Running), Err(InvalidTransition));
        }
        assert_eq!(
            JobState::Accepted.transition(JobState::Running),
            Ok(JobState::Running)
        );
        assert_eq!(
            JobState::Running.transition(JobState::Succeeded),
            Ok(JobState::Succeeded)
        );
    }

    #[test]
    fn late_results_cannot_cross_purge_or_replacement_generation() {
        let asset = AssetId::new();
        let fence = PublicationFence {
            asset,
            generation: 7,
        };
        assert!(fence.permits(asset, 7, false));
        assert!(!fence.permits(asset, 7, true));
        assert!(!fence.permits(asset, 8, false));
        assert!(!fence.permits(AssetId::new(), 7, false));
    }
}
