//! `GET` and `POST /api/preferences`: the dashboard's preferences in Kossé's configuration file.
//!
//! The read is answered from the file (defaults when there is none yet; `409 configUnavailable`
//! when it is corrupt or from a newer Kossé). The write passes the same guard as every unsafe
//! request (exact Origin, JSON, size limit, action token), has its own one-per-second limit, and
//! goes through the store's lock and atomic replace, keeping every other part of the file. A file
//! the store will not overwrite is left exactly as it is.

use std::sync::Arc;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use kosse_core::{
    contract::{ApiErrorCode, Preferences},
    profiles::CentralStore,
};

use crate::{
    api::RateLimit,
    customize::{error, store_failure, unfinished},
};

#[derive(Clone)]
pub struct PreferencesApi {
    store: CentralStore,
    limit: Arc<RateLimit>,
}

impl PreferencesApi {
    pub fn new(store: CentralStore) -> Self {
        Self {
            store,
            limit: Arc::new(RateLimit::default()),
        }
    }
}

pub async fn read(State(api): State<PreferencesApi>) -> Response {
    let store = api.store.clone();
    match tokio::task::spawn_blocking(move || store.preferences()).await {
        Ok(Ok(preferences)) => Json(preferences).into_response(),
        Ok(Err(failure)) => store_failure(failure),
        Err(_) => unfinished(),
    }
}

pub async fn save(
    State(api): State<PreferencesApi>,
    body: Result<Json<Preferences>, JsonRejection>,
) -> Response {
    let preferences = match body {
        Ok(Json(preferences)) => preferences,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return error(
                StatusCode::PAYLOAD_TOO_LARGE,
                ApiErrorCode::PayloadTooLarge,
                "request body too large".into(),
            );
        }
        Err(rejection) => {
            return error(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidPayload,
                rejection.body_text(),
            );
        }
    };
    if !api.limit.try_acquire() {
        let mut response = error(
            StatusCode::TOO_MANY_REQUESTS,
            ApiErrorCode::RateLimited,
            "preferences were saved less than a second ago".into(),
        );
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        return response;
    }
    let store = api.store.clone();
    match tokio::task::spawn_blocking(move || store.save_preferences(preferences)).await {
        Ok(Ok(stored)) => Json(stored).into_response(),
        Ok(Err(failure)) => store_failure(failure),
        Err(_) => unfinished(),
    }
}
