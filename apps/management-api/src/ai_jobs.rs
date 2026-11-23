use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use tm_trading_contracts::{AiJobDto, AiJobListDto, AiJobStatusDto, AiJobTaskDto, NewAiJobDto};
use tm_trading_domain::{
    AiBriefingReference, AiBriefingReferenceKind, AiJob, AiJobId, AiTaskKind, MAX_AI_JOB_INPUTS,
    NewAiJob, ai_job_request_hash,
};
use tm_trading_storage::{AiJobQuery, CreateAiJobOutcome, DEFAULT_AI_JOB_LIMIT};

use crate::auth::CAPABILITY_AI_READ;
use crate::auth::CAPABILITY_AI_WRITE;
use crate::error::ApiError;
use crate::state::AppState;

const MAX_LIMIT: usize = 200;
/// Lifetime of a queued job. The request is dropped instead of running much later.
const JOB_LIFETIME_HOURS: i64 = 1;
/// Maximum length of a client-supplied idempotency key.
const MAX_IDEMPOTENCY_KEY_CHARS: usize = 200;

/// Lists the authenticated owner's AI jobs, newest first.
pub async fn list_ai_jobs(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<AiJobListDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    state
        .ai_jobs
        .expire_due(Utc::now())
        .await
        .map_err(ApiError::from)?;
    let limit = match query.get("limit") {
        Some(value) => value.parse::<usize>().map_err(|_| {
            ApiError::BadRequest("limit must be an integer between 1 and 200".to_owned())
        })?,
        None => DEFAULT_AI_JOB_LIMIT,
    };
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::UnprocessableEntity(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let task = query
        .get("task")
        .map(|value| {
            if value == "research_explanation" {
                Ok(AiTaskKind::ResearchExplanation)
            } else {
                Err(ApiError::UnprocessableEntity(
                    "task is not a supported AI job task".to_owned(),
                ))
            }
        })
        .transpose()?;
    let jobs = state
        .ai_jobs
        .list(&AiJobQuery {
            owner_scope: auth.subject().to_owned(),
            task,
            limit,
        })
        .await
        .map_err(ApiError::from)?;
    let jobs = jobs.iter().map(to_dto).collect::<Result<Vec<_>, _>>()?;
    Ok(Json(AiJobListDto {
        generated_at: Utc::now(),
        count: jobs.len(),
        jobs,
    }))
}

/// Queues one evidence-bound briefing request for the authenticated owner.
pub async fn create_ai_job(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    body: Result<Json<NewAiJobDto>, JsonRejection>,
) -> Result<(StatusCode, Json<AiJobDto>), ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    let Json(body) = body.map_err(ApiError::from)?;
    let task = to_domain_task(body.task);
    if body
        .research_event_ids
        .len()
        .saturating_add(body.news_item_ids.len())
        > MAX_AI_JOB_INPUTS
    {
        return Err(ApiError::UnprocessableEntity(format!(
            "an AI job may contain at most {MAX_AI_JOB_INPUTS} combined inputs"
        )));
    }
    let research_event_ids = validate_ids(
        &body.research_event_ids,
        AiBriefingReferenceKind::ResearchEvent,
    )?;
    let news_item_ids = validate_ids(&body.news_item_ids, AiBriefingReferenceKind::NewsItem)?;
    if research_event_ids.is_empty() && news_item_ids.is_empty() {
        return Err(ApiError::UnprocessableEntity(
            "an AI job must name at least one research event or news item".to_owned(),
        ));
    }

    let request_hash = ai_job_request_hash(task, &research_event_ids, &news_item_ids);
    let idempotency_key = match body.idempotency_key {
        Some(value) => {
            let value = value.trim().to_owned();
            if value.is_empty() {
                return Err(ApiError::UnprocessableEntity(
                    "idempotency_key must not be empty".to_owned(),
                ));
            }
            if value.chars().count() > MAX_IDEMPOTENCY_KEY_CHARS {
                return Err(ApiError::UnprocessableEntity(format!(
                    "idempotency_key must be at most {MAX_IDEMPOTENCY_KEY_CHARS} characters"
                )));
            }
            value
        }
        None => request_hash.clone(),
    };

    let requested_at = Utc::now();
    let job = AiJob::new(NewAiJob {
        owner_scope: auth.subject().to_owned(),
        idempotency_key,
        request_hash,
        correlation_id: uuid::Uuid::new_v4(),
        task,
        research_event_ids,
        news_item_ids,
        requested_at,
        expires_at: requested_at + Duration::hours(JOB_LIFETIME_HOURS),
    })
    .map_err(|error| ApiError::UnprocessableEntity(error.to_string()))?;

    let outcome = state.ai_jobs.create(job).await.map_err(ApiError::from)?;
    match outcome {
        CreateAiJobOutcome::Created(job) => Ok((StatusCode::CREATED, Json(to_dto(&job)?))),
        CreateAiJobOutcome::Existing(job) => Ok((StatusCode::OK, Json(to_dto(&job)?))),
    }
}

/// Returns one AI job owned by the authenticated subject.
pub async fn get_ai_job(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(job_id): Path<String>,
) -> Result<Json<AiJobDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    state
        .ai_jobs
        .expire_due(Utc::now())
        .await
        .map_err(ApiError::from)?;
    let Some(job_id) = AiJobId::parse(&job_id) else {
        return Err(ApiError::NotFound("AI job was not found".to_owned()));
    };
    let job = state
        .ai_jobs
        .get(&job_id)
        .await
        .map_err(ApiError::from)?
        .filter(|job| job.owner_scope() == auth.subject())
        .ok_or_else(|| ApiError::NotFound("AI job was not found".to_owned()))?;
    Ok(Json(to_dto(&job)?))
}

fn validate_ids(values: &[String], kind: AiBriefingReferenceKind) -> Result<Vec<String>, ApiError> {
    let mut unique = Vec::with_capacity(values.len());
    let mut seen = HashSet::with_capacity(values.len());
    for value in values {
        let reference = AiBriefingReference::new(kind, value.trim()).map_err(|_| {
            ApiError::UnprocessableEntity(format!(
                "a valid {} identifier is required",
                kind.as_str()
            ))
        })?;
        let value = reference.value().to_owned();
        if seen.insert(value.clone()) {
            unique.push(value);
        }
    }
    Ok(unique)
}

fn to_domain_task(task: AiJobTaskDto) -> AiTaskKind {
    match task {
        AiJobTaskDto::ResearchExplanation => AiTaskKind::ResearchExplanation,
    }
}

fn to_dto(job: &AiJob) -> Result<AiJobDto, ApiError> {
    let task = match job.task() {
        AiTaskKind::ResearchExplanation => AiJobTaskDto::ResearchExplanation,
        _ => {
            return Err(ApiError::Internal(
                "stored AI job task is not supported by the public contract".to_owned(),
            ));
        }
    };
    Ok(AiJobDto {
        id: job.id().to_string(),
        owner_scope: job.owner_scope().to_owned(),
        correlation_id: job.correlation_id().to_string(),
        task,
        status: match job.status() {
            tm_trading_domain::AiJobStatus::Queued => AiJobStatusDto::Queued,
            tm_trading_domain::AiJobStatus::Claimed => AiJobStatusDto::Claimed,
            tm_trading_domain::AiJobStatus::Succeeded => AiJobStatusDto::Succeeded,
            tm_trading_domain::AiJobStatus::Failed => AiJobStatusDto::Failed,
            tm_trading_domain::AiJobStatus::Expired => AiJobStatusDto::Expired,
            tm_trading_domain::AiJobStatus::OutcomeUnknown => AiJobStatusDto::OutcomeUnknown,
        },
        attempt: job.attempt(),
        research_event_ids: job.research_event_ids().to_vec(),
        news_item_ids: job.news_item_ids().to_vec(),
        requested_at: job.requested_at(),
        expires_at: job.expires_at(),
        finished_at: job.finished_at(),
        briefing_id: job.briefing_id().map(str::to_owned),
        error_code: job.error_code().map(str::to_owned),
    })
}
