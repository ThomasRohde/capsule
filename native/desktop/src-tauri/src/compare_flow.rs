//! Trusted-shell, opaque comparison-session state.
//!
//! The WebView receives bounded projections and random capabilities only.
//! Source paths, retained SQLite snapshots, numeric contract positions and
//! canonical continuation cursors never cross the Rust boundary.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sqlite_capsule_workspace::{
    CancellationToken, CompareApplicationDetail, CompareApplicationLimits, CompareCursor,
    CompareDetailLimits, CompareRowDetail, CompareSummary, Sensitivity, VerifiedWorkspaceSource,
    WorkspaceError, WorkspaceErrorCode, compare_application_detail, comparison_detail_page,
};

use crate::reconcile_flow::{CompareReconcileHandoff, ReconcilePageBinding};

pub(crate) const COMPARE_CANDIDATE_PROFILE: &str = "org.sqlite-capsule.tauri-compare-candidate/1";
pub(crate) const COMPARE_SESSION_PROFILE: &str = "org.sqlite-capsule.tauri-compare-session/1";
pub(crate) const COMPARE_PAGE_PROFILE: &str = "org.sqlite-capsule.compare-page/1";
pub(crate) const CANDIDATE_LIFETIME: Duration = Duration::from_secs(5 * 60);
pub(crate) const COMPARE_OPERATION_LIFETIME: Duration = Duration::from_secs(30);
const MAX_ACTIVE_PAGE_CURSORS: usize = 8;

pub(crate) fn remaining_compare_lifetime(deadline: Instant) -> Result<Duration, WorkspaceError> {
    remaining_compare_lifetime_at(deadline, Instant::now())
}

fn remaining_compare_lifetime_at(
    deadline: Instant,
    now: Instant,
) -> Result<Duration, WorkspaceError> {
    deadline
        .checked_duration_since(now)
        .filter(|remaining| !remaining.is_zero())
        .filter(|remaining| *remaining <= COMPARE_OPERATION_LIFETIME)
        .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::LimitExceeded))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChooseCompareCandidateRequest {
    pub selection_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StartCompareRequest {
    pub selection_id: String,
    pub candidate_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComparePageRequest {
    pub session_id: String,
    pub table_token: String,
    #[serde(default)]
    pub page_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CloseCompareSessionRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompareApplicationRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CompareCandidateView {
    pub profile: &'static str,
    pub candidate_id: String,
    pub display: &'static str,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CompareTableSelectorView {
    pub table_label: String,
    pub table_token: String,
    pub sensitivity: Sensitivity,
    pub detail_available: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CompareDatasetSelectorView {
    pub dataset_label: String,
    pub dataset_token: String,
    pub tables: Vec<CompareTableSelectorView>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CompareSessionView {
    pub profile: &'static str,
    pub session_id: String,
    pub expires_at: String,
    pub report: CompareSummary,
    pub selectors: Vec<CompareDatasetSelectorView>,
}

#[derive(Serialize)]
pub(crate) struct ComparePageView {
    pub profile: &'static str,
    pub session_id: String,
    pub report_digest: String,
    pub dataset_label: String,
    pub table_label: String,
    pub sensitivity: Sensitivity,
    pub revealed: bool,
    pub rows: Vec<CompareRowDetail>,
    pub next_page_token: Option<String>,
    pub expires_at: String,
}

struct CandidateAuthority {
    selection_id: String,
    path: PathBuf,
    view: CompareCandidateView,
    deadline: Instant,
}

struct TableAuthority {
    dataset_index: usize,
    table_index: usize,
    sensitivity: Sensitivity,
    detail_available: bool,
}

struct CursorAuthority {
    table_token: String,
    revealed: bool,
    cursor: CompareCursor,
}

struct CompareSession {
    selection_id: String,
    left: VerifiedWorkspaceSource,
    right: VerifiedWorkspaceSource,
    view: CompareSessionView,
    tables: BTreeMap<String, TableAuthority>,
    cursors: BTreeMap<String, CursorAuthority>,
    deadline: Instant,
}

#[derive(Default)]
pub(crate) struct CompareController {
    candidate: Option<CandidateAuthority>,
    session: Option<CompareSession>,
}

#[derive(Clone, Default)]
pub(crate) struct CompareState(
    pub Arc<Mutex<CompareController>>,
    pub Arc<Mutex<Option<ActiveCompareRequest>>>,
);

pub(crate) struct ActiveCompareRequest {
    pub session_id: String,
    pub cancellation: CancellationToken,
}

impl ActiveCompareRequest {
    pub(crate) fn new(
        session_id: &str,
        cancellation: CancellationToken,
    ) -> Result<Self, WorkspaceError> {
        validate_token(session_id)?;
        Ok(Self {
            session_id: session_id.to_owned(),
            cancellation,
        })
    }
}

impl CompareController {
    pub(crate) fn invalidate_selection(&mut self, current_selection: Option<&str>) {
        if self
            .candidate
            .as_ref()
            .is_some_and(|candidate| Some(candidate.selection_id.as_str()) != current_selection)
        {
            self.candidate = None;
        }
        if self
            .session
            .as_ref()
            .is_some_and(|session| Some(session.selection_id.as_str()) != current_selection)
        {
            self.session = None;
        }
    }

    pub(crate) fn retain_candidate(
        &mut self,
        selection_id: &str,
        path: PathBuf,
        candidate_id: String,
        expires_at: String,
    ) -> Result<CompareCandidateView, WorkspaceError> {
        validate_token(selection_id)?;
        validate_token(&candidate_id)?;
        let view = CompareCandidateView {
            profile: COMPARE_CANDIDATE_PROFILE,
            candidate_id,
            display: "Selected Capsule",
            expires_at,
        };
        self.session = None;
        self.candidate = Some(CandidateAuthority {
            selection_id: selection_id.to_owned(),
            path,
            view: view.clone(),
            deadline: Instant::now() + CANDIDATE_LIFETIME,
        });
        Ok(view)
    }

    pub(crate) fn consume_candidate(
        &mut self,
        selection_id: &str,
        candidate_id: &str,
    ) -> Result<PathBuf, WorkspaceError> {
        validate_token(selection_id)?;
        validate_token(candidate_id)?;
        let candidate = self
            .candidate
            .take()
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if Instant::now() >= candidate.deadline {
            return Err(WorkspaceError::new(WorkspaceErrorCode::SessionExpired));
        }
        if candidate.selection_id != selection_id || candidate.view.candidate_id != candidate_id {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        Ok(candidate.path)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retain_session(
        &mut self,
        selection_id: String,
        session_id: String,
        expires_at: String,
        deadline: Instant,
        left: VerifiedWorkspaceSource,
        right: VerifiedWorkspaceSource,
        report: CompareSummary,
    ) -> Result<CompareSessionView, WorkspaceError> {
        validate_token(&selection_id)?;
        validate_token(&session_id)?;
        remaining_compare_lifetime(deadline)?;
        let mut tables = BTreeMap::new();
        let mut selectors = Vec::with_capacity(report.datasets.len());
        for (dataset_index, dataset) in report.datasets.iter().enumerate() {
            let dataset_token = random_token()?;
            let mut table_views = Vec::with_capacity(dataset.tables.len());
            for (table_index, table) in dataset.tables.iter().enumerate() {
                let table_token = random_token()?;
                let detail_available = report.compatibility.can_compare_data
                    && !table.truncated
                    && matches!(
                        dataset.policy,
                        sqlite_capsule_workspace::ComparePolicy::Row
                            | sqlite_capsule_workspace::ComparePolicy::Field
                    );
                tables.insert(
                    table_token.clone(),
                    TableAuthority {
                        dataset_index,
                        table_index,
                        sensitivity: dataset.sensitivity,
                        detail_available,
                    },
                );
                table_views.push(CompareTableSelectorView {
                    table_label: table.table.clone(),
                    table_token,
                    sensitivity: dataset.sensitivity,
                    detail_available,
                });
            }
            selectors.push(CompareDatasetSelectorView {
                dataset_label: dataset.dataset_id.clone(),
                dataset_token,
                tables: table_views,
            });
        }
        let view = CompareSessionView {
            profile: COMPARE_SESSION_PROFILE,
            session_id,
            expires_at,
            report,
            selectors,
        };
        self.candidate = None;
        self.session = Some(CompareSession {
            selection_id,
            left,
            right,
            view: view.clone(),
            tables,
            cursors: BTreeMap::new(),
            deadline,
        });
        Ok(view)
    }

    pub(crate) fn detail_page(
        &mut self,
        request: &ComparePageRequest,
        reveal_sensitive: bool,
        cancellation: &CancellationToken,
    ) -> Result<ComparePageView, WorkspaceError> {
        validate_token(&request.session_id)?;
        validate_token(&request.table_token)?;
        if let Some(token) = &request.page_token {
            validate_token(token)?;
        }
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if Instant::now() >= session.deadline {
            self.session = None;
            return Err(WorkspaceError::new(WorkspaceErrorCode::SessionExpired));
        }
        if session.view.session_id != request.session_id {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        let table = session
            .tables
            .get(&request.table_token)
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if !table.detail_available {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedOperation,
            ));
        }
        if table.sensitivity == Sensitivity::Sensitive && !reveal_sensitive {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedOperation,
            ));
        }
        let cursor = match &request.page_token {
            None => None,
            Some(token) => {
                let authority = session
                    .cursors
                    .remove(token)
                    .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
                if authority.table_token != request.table_token
                    || authority.revealed != reveal_sensitive
                {
                    return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
                }
                Some(authority.cursor)
            }
        };
        let remaining = session
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::SessionExpired))?;
        let page = comparison_detail_page(
            &session.left,
            &session.right,
            table.dataset_index,
            table.table_index,
            cursor,
            reveal_sensitive,
            &CompareDetailLimits {
                deadline: remaining,
                ..CompareDetailLimits::default()
            },
            cancellation,
        )?;
        let next_page_token = match page.next_cursor {
            Some(cursor) => {
                if session.cursors.len() >= MAX_ACTIVE_PAGE_CURSORS {
                    return Err(WorkspaceError::new(WorkspaceErrorCode::LimitExceeded));
                }
                let token = random_token()?;
                session.cursors.insert(
                    token.clone(),
                    CursorAuthority {
                        table_token: request.table_token.clone(),
                        revealed: reveal_sensitive,
                        cursor,
                    },
                );
                Some(token)
            }
            None => None,
        };
        Ok(ComparePageView {
            profile: COMPARE_PAGE_PROFILE,
            session_id: session.view.session_id.clone(),
            report_digest: session.view.report.report_digest.clone(),
            dataset_label: page.dataset_label,
            table_label: page.table_label,
            sensitivity: page.sensitivity,
            revealed: page.revealed,
            rows: page.rows,
            next_page_token,
            expires_at: session.view.expires_at.clone(),
        })
    }

    pub(crate) fn application_detail(
        &mut self,
        request: &CompareApplicationRequest,
        cancellation: &CancellationToken,
    ) -> Result<CompareApplicationDetail, WorkspaceError> {
        validate_token(&request.session_id)?;
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if Instant::now() >= session.deadline {
            self.session = None;
            return Err(WorkspaceError::new(WorkspaceErrorCode::SessionExpired));
        }
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if session.view.session_id != request.session_id {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        let remaining = session
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::SessionExpired))?;
        compare_application_detail(
            &session.left,
            &session.right,
            &session.view.report,
            &CompareApplicationLimits {
                operation_deadline: Some(remaining),
                ..CompareApplicationLimits::default()
            },
            cancellation,
        )
    }

    pub(crate) fn reconcile_page_binding(
        &mut self,
        request: &ComparePageRequest,
    ) -> Result<ReconcilePageBinding, WorkspaceError> {
        validate_token(&request.session_id)?;
        validate_token(&request.table_token)?;
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if Instant::now() >= session.deadline {
            self.session = None;
            return Err(WorkspaceError::new(WorkspaceErrorCode::SessionExpired));
        }
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if session.view.session_id != request.session_id {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        let authority = session
            .tables
            .get(&request.table_token)
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if !authority.detail_available {
            return Err(WorkspaceError::new(
                WorkspaceErrorCode::UnsupportedOperation,
            ));
        }
        let dataset = session
            .left
            .data_contract()
            .datasets
            .get(authority.dataset_index)
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::InvalidContract))?;
        let table = dataset
            .tables
            .get(authority.table_index)
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::InvalidContract))?;
        Ok(ReconcilePageBinding {
            selection_id: session.selection_id.clone(),
            session_id: session.view.session_id.clone(),
            report_digest: session.view.report.report_digest.clone(),
            dataset_index: authority.dataset_index,
            table_index: authority.table_index,
            dataset_label: dataset.id.clone(),
            table_label: table.name.clone(),
            compare_policy: dataset.compare,
            reconcile_policy: dataset.reconcile,
            sensitivity: dataset.sensitivity,
            table: table.clone(),
        })
    }

    pub(crate) fn take_for_reconcile(
        &mut self,
        session_id: &str,
        report_digest: &str,
    ) -> Result<CompareReconcileHandoff, WorkspaceError> {
        validate_token(session_id)?;
        if report_digest.len() != 64
            || !report_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidContract));
        }
        let session = self
            .session
            .take()
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if Instant::now() >= session.deadline {
            return Err(WorkspaceError::new(WorkspaceErrorCode::SessionExpired));
        }
        if session.view.session_id != session_id
            || session.view.report.report_digest != report_digest
        {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        Ok(CompareReconcileHandoff {
            selection_id: session.selection_id,
            session_id: session.view.session_id,
            report: session.view.report,
            left: session.left,
            right: session.right,
            compare_deadline: session.deadline,
        })
    }

    pub(crate) fn close_session(&mut self, session_id: &str) -> Result<(), WorkspaceError> {
        validate_token(session_id)?;
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| WorkspaceError::new(WorkspaceErrorCode::StalePlan))?;
        if session.view.session_id != session_id {
            return Err(WorkspaceError::new(WorkspaceErrorCode::StalePlan));
        }
        self.session = None;
        Ok(())
    }

    pub(crate) fn expire_session(&mut self, session_id: &str) -> bool {
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.view.session_id == session_id)
        {
            self.session = None;
            true
        } else {
            false
        }
    }
}

fn validate_token(value: &str) -> Result<(), WorkspaceError> {
    if value.len() != 43
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(WorkspaceError::new(WorkspaceErrorCode::InvalidContract));
    }
    Ok(())
}

fn random_token() -> Result<String, WorkspaceError> {
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random)
        .map_err(|_| WorkspaceError::new(WorkspaceErrorCode::InternalError))?;
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(43);
    for chunk in random.chunks(3) {
        let mut block = u32::from(chunk[0]) << 16;
        if let Some(value) = chunk.get(1) {
            block |= u32::from(*value) << 8;
        }
        if let Some(value) = chunk.get(2) {
            block |= u32::from(*value);
        }
        output.push(ALPHABET[((block >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 63) as usize] as char);
        if chunk.len() >= 2 {
            output.push(ALPHABET[((block >> 6) & 63) as usize] as char);
        }
        if chunk.len() == 3 {
            output.push(ALPHABET[(block & 63) as usize] as char);
        }
    }
    validate_token(&output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_are_opaque_one_use_and_selection_bound() {
        let mut controller = CompareController::default();
        let selection = "A".repeat(43);
        let candidate = "B".repeat(43);
        let path = PathBuf::from(r"C:\private\never-rendered.sqlitecapsule");
        let view = controller
            .retain_candidate(
                &selection,
                path.clone(),
                candidate.clone(),
                "2026-08-13T07:45:00Z".to_owned(),
            )
            .unwrap();
        let serialized = serde_json::to_string(&view).unwrap();
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains("sqlitecapsule"));
        assert_eq!(
            controller
                .consume_candidate(&"C".repeat(43), &candidate)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::StalePlan
        );
        assert!(controller.candidate.is_none());

        controller
            .retain_candidate(
                &selection,
                path.clone(),
                candidate.clone(),
                "2026-08-13T07:45:00Z".to_owned(),
            )
            .unwrap();
        assert_eq!(
            controller
                .consume_candidate(&selection, &candidate)
                .unwrap(),
            path
        );
        assert_eq!(
            controller
                .consume_candidate(&selection, &candidate)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::StalePlan
        );
    }

    #[test]
    fn invalid_tokens_and_selection_changes_fail_before_path_resolution() {
        let mut controller = CompareController::default();
        assert_eq!(
            controller
                .retain_candidate(
                    "short",
                    PathBuf::from("ignored"),
                    "B".repeat(43),
                    "2026-08-13T07:45:00Z".to_owned(),
                )
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::InvalidContract
        );
        controller
            .retain_candidate(
                &"A".repeat(43),
                PathBuf::from("private"),
                "B".repeat(43),
                "2026-08-13T07:45:00Z".to_owned(),
            )
            .unwrap();
        controller.invalidate_selection(Some(&"C".repeat(43)));
        assert!(controller.candidate.is_none());
    }

    #[test]
    fn active_requests_are_exactly_session_bound() {
        let cancellation = CancellationToken::new();
        let active = ActiveCompareRequest::new(&"A".repeat(43), cancellation.clone()).unwrap();
        assert_eq!(active.session_id, "A".repeat(43));
        assert!(!active.cancellation.is_cancelled());
        assert_eq!(
            ActiveCompareRequest::new("short", CancellationToken::new())
                .err()
                .expect("short session must fail")
                .kind(),
            WorkspaceErrorCode::InvalidContract
        );
    }

    #[test]
    fn delayed_session_handoff_retains_one_absolute_deadline() {
        let started = Instant::now();
        let deadline = started
            .checked_add(COMPARE_OPERATION_LIFETIME)
            .expect("bounded deadline");
        let delayed_handoff = started
            .checked_add(Duration::from_secs(7))
            .expect("bounded delayed handoff");
        assert_eq!(
            remaining_compare_lifetime_at(deadline, delayed_handoff).unwrap(),
            Duration::from_secs(23)
        );
        assert_eq!(
            remaining_compare_lifetime_at(deadline, deadline)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::LimitExceeded
        );
        assert_eq!(
            deadline.saturating_duration_since(delayed_handoff),
            Duration::from_secs(23),
            "the reaper must wait only until the original deadline"
        );
        let mut controller = CompareController::default();
        assert!(!controller.expire_session(&"A".repeat(43)));
    }

    #[test]
    fn reconcile_transfer_requires_live_compare_then_uses_a_separate_wrapper() {
        let started = Instant::now();
        let compare_deadline = started + COMPARE_OPERATION_LIFETIME;
        let transfer_time = started + Duration::from_secs(29);
        assert_eq!(
            remaining_compare_lifetime_at(compare_deadline, transfer_time).unwrap(),
            Duration::from_secs(1),
            "the handoff cannot refresh or outlive compare authority"
        );
        assert_eq!(
            remaining_compare_lifetime_at(compare_deadline, compare_deadline)
                .unwrap_err()
                .kind(),
            WorkspaceErrorCode::LimitExceeded,
            "an expired comparison cannot be transferred"
        );
        let human_deadline = transfer_time + crate::reconcile_flow::HUMAN_REVIEW_LIFETIME;
        assert_eq!(
            human_deadline.saturating_duration_since(transfer_time),
            Duration::from_secs(5 * 60),
            "only the new opaque human-review wrapper receives five minutes"
        );
        assert!(human_deadline > compare_deadline);
    }
}
