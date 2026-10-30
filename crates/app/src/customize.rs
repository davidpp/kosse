//! `POST /api/profile` ("Customize this project"): saves or removes a project's central profile.
//!
//! It passes the same guard as every unsafe request (exact Origin, JSON, size limit, action token)
//! and has its own one-per-second limit, separate from refresh, probe and stop. Only Kossé's own
//! configuration file is written, never the project: no repository file, `.gitignore` or Git
//! state is touched. The root must be a project in the current snapshot or one that already has a
//! central profile, so the dashboard cannot be used to plant profiles for arbitrary paths. The
//! scanner picks the change up on the scan this triggers, and every view sees it in the next
//! snapshot. A request naming an `expectedRevision` is refused (`409 revisionConflict`) when
//! another writer, such as an agent's `kosse profile set`, changed the profile first.
//!
//! `GET /api/profile/prompt?root=<root>` returns the customization prompt for a known root, the
//! same text `kosse profile prompt` prints (core generates both). It is a read: it writes nothing
//! and passes the guard like every route.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use kosse_core::{
    contract::{ApiError, ApiErrorCode, ProfilePrompt, SaveProfileRequest, SavedProfile},
    profiles::{CentralStore, StoreError, central, prompt, view::ProfileSource},
};
use serde::Deserialize;

use crate::{api::RateLimit, scheduler::SchedulerHandle};

#[derive(Clone)]
pub struct Customize {
    scheduler: SchedulerHandle,
    store: CentralStore,
    limit: Arc<RateLimit>,
}

impl Customize {
    pub fn new(scheduler: SchedulerHandle, store: CentralStore) -> Self {
        Self {
            scheduler,
            store,
            limit: Arc::new(RateLimit::default()),
        }
    }

    pub fn store(&self) -> CentralStore {
        self.store.clone()
    }

    /// A project of the latest snapshot, or a root with a saved central profile.
    fn knows(&self, root: &str) -> bool {
        let Some(snapshot) = self.scheduler.latest() else {
            return false;
        };
        snapshot.observation.as_ref().is_some_and(|observation| {
            observation
                .projects
                .iter()
                .any(|project| project.root == root)
                || observation.profiles.iter().any(|metadata| {
                    metadata.root == root && metadata.sources.contains(&ProfileSource::Central)
                })
        })
    }
}

#[derive(Deserialize)]
pub struct PromptQuery {
    root: String,
}

pub async fn prompt(
    State(customize): State<Customize>,
    query: Result<Query<PromptQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(PromptQuery { root })) = query else {
        return error(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidPayload,
            "expected ?root=<project root>".into(),
        );
    };
    if !customize.knows(&root) {
        return unknown_project();
    }
    match prompt::customization(&root) {
        Ok(prompt) => Json(ProfilePrompt { root, prompt }).into_response(),
        Err(refused) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            ApiErrorCode::UnsafeRoot,
            refused.to_string(),
        ),
    }
}

fn unknown_project() -> Response {
    error(
        StatusCode::NOT_FOUND,
        ApiErrorCode::UnknownProject,
        "not a project in the current snapshot, and no profile is saved for it".into(),
    )
}

pub async fn save(
    State(customize): State<Customize>,
    body: Result<Json<SaveProfileRequest>, JsonRejection>,
) -> Response {
    let request = match body {
        Ok(Json(request)) => request,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return error(
                StatusCode::PAYLOAD_TOO_LARGE,
                ApiErrorCode::PayloadTooLarge,
                "request body too large".into(),
            );
        }
        // The extractor's text says which field failed; it quotes only what the client sent.
        Err(rejection) => {
            return error(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidPayload,
                rejection.body_text(),
            );
        }
    };
    if !customize.knows(&request.root) {
        return unknown_project();
    }
    if !customize.limit.try_acquire() {
        let mut response = error(
            StatusCode::TOO_MANY_REQUESTS,
            ApiErrorCode::RateLimited,
            "a profile was saved less than a second ago".into(),
        );
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        return response;
    }

    let store = customize.store.clone();
    let root = request.root.clone();
    let expected = request.expected_revision;
    let saved = tokio::task::spawn_blocking(move || {
        store.save(&root, request.profile, expected.as_deref())
    })
    .await;
    match saved {
        Ok(Ok(profile)) => {
            // The snapshot stream delivers the result; a busy scanner only delays it.
            let scheduler = customize.scheduler.clone();
            tokio::spawn(async move {
                let _ = scheduler.refresh().await;
            });
            Json(SavedProfile {
                root: request.root,
                revision: central::revision(profile.as_ref()),
                profile,
            })
            .into_response()
        }
        Ok(Err(failure)) => store_failure(failure),
        Err(_) => unfinished(),
    }
}

/// `422 invalidProfile`, `409 revisionConflict`, `409 configUnavailable` (an unusable or full
/// file, left as it is) or `500 configWriteFailed`.
pub(crate) fn store_failure(failure: StoreError) -> Response {
    match failure {
        failure @ StoreError::RevisionMismatch { .. } => error(
            StatusCode::CONFLICT,
            ApiErrorCode::RevisionConflict,
            failure.to_string(),
        ),
        StoreError::Rejected(problem) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            ApiErrorCode::InvalidProfile,
            problem.to_string(),
        ),
        failure @ (StoreError::Unusable { .. } | StoreError::TooLarge) => error(
            StatusCode::CONFLICT,
            ApiErrorCode::ConfigUnavailable,
            failure.to_string(),
        ),
        failure => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::ConfigWriteFailed,
            failure.to_string(),
        ),
    }
}

/// The blocking configuration task panicked or was cancelled.
pub(crate) fn unfinished() -> Response {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiErrorCode::ConfigWriteFailed,
        "the configuration write did not finish".into(),
    )
}

pub(crate) fn error(status: StatusCode, code: ApiErrorCode, message: String) -> Response {
    (
        status,
        Json(ApiError {
            error: code,
            message,
        }),
    )
        .into_response()
}
