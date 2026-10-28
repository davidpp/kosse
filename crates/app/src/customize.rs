//! `POST /api/profile` ("Customize this project"): saves or removes a project's central profile.
//!
//! It passes the same guard as every unsafe request (exact Origin, JSON, size limit, action token)
//! and has its own one-per-second limit, separate from refresh, probe and stop. Only Kossé's own
//! configuration file is written, never the project: no repository file, `.gitignore` or Git
//! state is touched. The root must be a project in the current snapshot or one that already has a
//! central profile, so the dashboard cannot be used to plant profiles for arbitrary paths. The
//! scanner picks the change up on the scan this triggers, and every view sees it in the next
//! snapshot.

use std::sync::Arc;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use kosse_core::{
    contract::{ApiError, ApiErrorCode, SaveProfileRequest, SavedProfile},
    profiles::{CentralStore, StoreError, view::ProfileSource},
};

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
        return error(
            StatusCode::NOT_FOUND,
            ApiErrorCode::UnknownProject,
            "not a project in the current snapshot, and no profile is saved for it".into(),
        );
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
    let saved = tokio::task::spawn_blocking(move || store.save(&root, request.profile)).await;
    match saved {
        Ok(Ok(profile)) => {
            // The snapshot stream delivers the result; a busy scanner only delays it.
            let scheduler = customize.scheduler.clone();
            tokio::spawn(async move {
                let _ = scheduler.refresh().await;
            });
            Json(SavedProfile {
                root: request.root,
                profile,
            })
            .into_response()
        }
        Ok(Err(StoreError::Rejected(problem))) => error(
            StatusCode::UNPROCESSABLE_ENTITY,
            ApiErrorCode::InvalidProfile,
            problem.to_string(),
        ),
        Ok(Err(failure @ (StoreError::Unusable { .. } | StoreError::TooLarge))) => error(
            StatusCode::CONFLICT,
            ApiErrorCode::ConfigUnavailable,
            failure.to_string(),
        ),
        Ok(Err(failure)) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::ConfigWriteFailed,
            failure.to_string(),
        ),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::ConfigWriteFailed,
            "the configuration write did not finish".into(),
        ),
    }
}

fn error(status: StatusCode, code: ApiErrorCode, message: String) -> Response {
    (
        status,
        Json(ApiError {
            error: code,
            message,
        }),
    )
        .into_response()
}
