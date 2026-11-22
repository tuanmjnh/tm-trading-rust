use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::Utc;
use tm_trading_contracts::{
    AiBriefingDto, AiBriefingListDto, AiBriefingReferenceDto, AiBriefingReferenceKindDto,
    AiBriefingTaskDto,
};
use tm_trading_domain::{AiBriefing, AiBriefingReferenceKind, AiTaskKind, is_valid_ai_briefing_id};
use tm_trading_storage::AiBriefingQuery;

use crate::auth::CAPABILITY_AI_READ;
use crate::error::ApiError;
use crate::state::AppState;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

/// Lists the authenticated owner's AI briefings, newest first.
pub async fn list_ai_briefings(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<AiBriefingListDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    let limit = match query.get("limit") {
        Some(value) => value.parse::<usize>().map_err(|_| {
            ApiError::BadRequest("limit must be an integer between 1 and 200".to_owned())
        })?,
        None => DEFAULT_LIMIT,
    };
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::UnprocessableEntity(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let task = query
        .get("task")
        .map(|value| {
            AiTaskKind::parse(value)
                .filter(|task| task.as_str() == value.as_str())
                .ok_or_else(|| {
                    ApiError::UnprocessableEntity("task is not a supported AI task".to_owned())
                })
        })
        .transpose()?;
    let briefings = state
        .ai_briefings
        .list(&AiBriefingQuery {
            owner_scope: auth.subject().to_owned(),
            task,
            limit,
        })
        .await
        .map_err(ApiError::from)?;
    let briefings = briefings.iter().map(to_dto).collect::<Vec<_>>();
    Ok(Json(AiBriefingListDto {
        generated_at: Utc::now(),
        count: briefings.len(),
        briefings,
    }))
}

/// Returns one AI briefing owned by the authenticated subject.
pub async fn get_ai_briefing(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(briefing_id): Path<String>,
) -> Result<Json<AiBriefingDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    if !is_valid_ai_briefing_id(&briefing_id) {
        return Err(ApiError::NotFound("AI briefing was not found".to_owned()));
    }
    let briefing = state
        .ai_briefings
        .get(auth.subject(), &briefing_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("AI briefing was not found".to_owned()))?;
    Ok(Json(to_dto(&briefing)))
}

fn to_task_dto(task: AiTaskKind) -> AiBriefingTaskDto {
    match task {
        AiTaskKind::NewsClassification => AiBriefingTaskDto::NewsClassification,
        AiTaskKind::NewsSummary => AiBriefingTaskDto::NewsSummary,
        AiTaskKind::MarketBriefing => AiBriefingTaskDto::MarketBriefing,
        AiTaskKind::ResearchExplanation => AiBriefingTaskDto::ResearchExplanation,
        AiTaskKind::DailyDigest => AiBriefingTaskDto::DailyDigest,
    }
}

fn to_dto(briefing: &AiBriefing) -> AiBriefingDto {
    AiBriefingDto {
        id: briefing.id().to_owned(),
        owner_scope: briefing.owner_scope().to_owned(),
        task: to_task_dto(briefing.task()),
        provider_id: briefing.provider_id().to_owned(),
        model_registry_id: briefing.model_registry_id().to_owned(),
        model_id: briefing.model_id().to_owned(),
        prompt_version: briefing.prompt_version().to_owned(),
        facts: briefing.facts().to_vec(),
        interpretations: briefing.interpretations().to_vec(),
        uncertainties: briefing.uncertainties().to_vec(),
        input_snapshot_ids: briefing.input_snapshot_ids().to_vec(),
        references: briefing
            .references()
            .iter()
            .map(|reference| AiBriefingReferenceDto {
                kind: match reference.kind() {
                    AiBriefingReferenceKind::ResearchEvent => {
                        AiBriefingReferenceKindDto::ResearchEvent
                    }
                    AiBriefingReferenceKind::NewsItem => AiBriefingReferenceKindDto::NewsItem,
                    AiBriefingReferenceKind::SourceUrl => AiBriefingReferenceKindDto::SourceUrl,
                },
                value: reference.value().to_owned(),
            })
            .collect(),
        missing_inputs: briefing.missing_inputs().to_vec(),
        stale_inputs: briefing.stale_inputs().to_vec(),
        generated_at: briefing.generated_at(),
        producer_version: briefing.producer_version().to_owned(),
        schema_version: briefing.schema_version().to_owned(),
    }
}
