//! Cross-workspace and cross-organization container copy (`copy-to-workspace`).
//!
//! Copies a live `Board`, `Wiki`, or `Course` into another workspace opened by the
//! caller's session (including across organizations or into the caller's personal space),
//! respecting `organizations.allow_copy_out`, PostgreSQL RLS handle boundaries, and
//! target organization storage limits.

use std::collections::{HashMap, HashSet};

use fvoci_model::{
    Block, BlockError, BlockType, Body, CardPropertyDefinition, Container, ContainerKind,
    ContainerKinds, ContainerMember, ContainerVisibility, ContainersAndBlocks,
    CopiedContainerResult, CopyContainerToWorkspaceRequest, CourseScheduleUpdate, OrgKind,
    PageNode, PageTree, PropertyType, Renewed, Session, TEMPLATE_SCHEDULE_PROPERTY,
    ValidatedCourseSchedule, WorkspaceMember, WorkspaceRole, add_days,
    clear_values_of_another_kind, date_property_ids, is_id, move_dates,
    renew_containers_and_blocks_ids_told,
};
use fvoci_store::{CopyChanges, StreamBatch, StreamCreateError};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::accept_language::Reader;
use super::container_copies::{counted_as, titles_in};
use super::language::Language;
use super::loose_objects::WrittenPlace;
use super::pages::refused_without_member;
use super::templates::{TEMPLATE_MARKS, changes_of_copy};
use super::{App, MAX_REFERENCE_IDS};
use crate::auth::now_millis;
use crate::error::{AppError, Reason};
use crate::metrics::{ContainerWork, CourseSource};

const COPY_STREAM_BATCH: usize = 64;

struct SourceSnapshot {
    org_id: String,
    org_kind: OrgKind,
    container: Container,
    blocks: Vec<Block>,
    schedule: Option<CourseScheduleUpdate>,
    external_titles: HashMap<String, String>,
    member_names: HashMap<String, String>,
}

impl App {
    /// Copies `container_id` from `source_ws_id` into `request.target_workspace_id` in
    /// three strict steps across tenant handles:
    /// 1. Flush active Yjs rooms and read container, live non-comment blocks, schedule,
    ///    readable external reference titles, and member names on the source handle.
    /// 2. Check target storage room and pre-copy referenced storage objects under
    ///    `loose_objects` marks on the target handle.
    /// 3. Sanitize references/mentions/people and commit the container, blocks, file
    ///    records, schedule, and copier `Admin` membership in one target DB transaction.
    pub async fn copy_container_to_workspace(
        &self,
        session: &Session,
        source_ws_id: &str,
        container_id: &str,
        request: &CopyContainerToWorkspaceRequest,
        accept_language: Option<&str>,
    ) -> Result<CopiedContainerResult, AppError> {
        let target_ws_id = request.target_workspace_id.trim();
        if target_ws_id.is_empty() {
            return Err(AppError::bad_request("targetWorkspaceId is required")
                .with_field("targetWorkspaceId"));
        }
        if !is_id(target_ws_id) {
            return Err(AppError::not_found("workspace"));
        }

        let source_entered = self
            .enter_workspace(session, source_ws_id)
            .await?
            .ok_or_else(|| AppError::not_found("board"))?;
        let copyable_kinds = ContainerKinds::SHARED;
        let source_app = source_entered
            .app
            .clone()
            .in_workspace(source_ws_id)
            .reaching(copyable_kinds);

        let access = source_app
            .container_access(&session.user_id, container_id)
            .await?
            .ok_or_else(|| AppError::not_found("board"))?;
        if !access.grant.sees() || access.grant.manages_only() {
            return Err(AppError::not_found("board"));
        }
        if !access.actor.works_there()
            || (!access.container.is_template && !access.has_membership())
        {
            return Err(AppError::forbidden("access denied to copy board"));
        }

        let target_entered = self
            .enter_workspace(session, target_ws_id)
            .await?
            .ok_or_else(|| AppError::not_found("workspace"))?;
        if target_entered.role == WorkspaceRole::Guest {
            return Err(AppError::not_found("workspace"));
        }

        let source_org_id = source_app
            .store()
            .tenant()
            .org_id()
            .ok_or_else(|| AppError::not_found("board"))?;
        let target_org_id = target_entered
            .app
            .store()
            .tenant()
            .org_id()
            .ok_or_else(|| AppError::not_found("workspace"))?;

        if source_org_id != target_org_id && source_entered.kind == OrgKind::Shared {
            let org = source_entered.app.organization().await?;
            if !org.allow_copy_out {
                return Err(AppError::forbidden(
                    "copying containers outside this organization is disabled",
                )
                .with_reason(Reason::CopyOutDisabled));
            }
        }

        let snapshot = self
            .read_source_snapshot(
                &source_entered.app,
                &source_app,
                source_ws_id,
                &source_org_id,
                source_entered.kind,
                container_id,
                &session.user_id,
            )
            .await?;

        let target_app = target_entered
            .app
            .in_workspace(target_ws_id)
            .reaching(copyable_kinds);
        let result = self
            .stage_and_commit_workspace_copy(
                &source_app,
                &target_app,
                &target_org_id,
                target_ws_id,
                &session.user_id,
                request.title.as_deref(),
                accept_language,
                snapshot,
            )
            .await?;

        if source_org_id != target_org_id
            && source_entered.kind == OrgKind::Shared
            && let Err(error) = source_entered
                .app
                .store()
                .record_container_copied_out(&session.user_id, container_id, &target_org_id)
                .await
        {
            tracing::error!(
                container_id,
                target_org_id,
                %error,
                "failed to record container_copied_out audit entry"
            );
        }

        Ok(result)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "source snapshot collects tenant-scoped handles and caller context in one pass"
    )]
    async fn read_source_snapshot(
        &self,
        source_org_app: &Self,
        source_app: &Self,
        source_ws_id: &str,
        source_org_id: &str,
        org_kind: OrgKind,
        container_id: &str,
        user_id: &str,
    ) -> Result<SourceSnapshot, AppError> {
        source_app.write_container_bodies(container_id).await?;
        let container = source_app.container(container_id).await?;
        if container.kind == ContainerKind::Diary {
            return Err(AppError::not_found("board"));
        }
        let mut blocks = source_app.blocks_for_container(container_id).await?;
        blocks.retain(|b| b.block_type != BlockType::Comment && b.deleted_at.is_none());
        blocks.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));

        let schedule = read_source_course_schedule(source_app, &container).await?;
        let internal_ids: HashSet<&str> = blocks.iter().map(|b| b.id.as_str()).collect();
        let mut external_ref_ids = Vec::new();
        let mut seen_refs = HashSet::new();
        for block in &blocks {
            if let Some(body) = &block.body {
                for ref_id in body.reference_ids() {
                    if !internal_ids.contains(ref_id)
                        && is_id(ref_id)
                        && seen_refs.insert(ref_id.to_owned())
                    {
                        external_ref_ids.push(ref_id.to_owned());
                    }
                }
            }
        }
        let ref_app = source_org_app
            .clone()
            .in_workspace(source_ws_id)
            .reaching(ContainerKinds::EVERY);
        let mut external_titles = HashMap::new();
        for chunk in external_ref_ids.chunks(MAX_REFERENCE_IDS) {
            for linked in ref_app.resolve_references(user_id, chunk).await? {
                external_titles.insert(linked.id, linked.title);
            }
        }

        let members = source_org_app.workspace_members(source_ws_id).await?;
        let member_names = build_member_name_map(&members);

        Ok(SourceSnapshot {
            org_id: source_org_id.to_owned(),
            org_kind,
            container,
            blocks,
            schedule,
            external_titles,
            member_names,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "target staging and single-transaction commit require source/target handles and snapshot"
    )]
    async fn stage_and_commit_workspace_copy(
        &self,
        source_app: &Self,
        target_app: &Self,
        target_org_id: &str,
        target_ws_id: &str,
        user_id: &str,
        custom_title: Option<&str>,
        accept_language: Option<&str>,
        snapshot: SourceSnapshot,
    ) -> Result<CopiedContainerResult, AppError> {
        let now = now_millis();
        let reader = target_app.reader_at(user_id, accept_language, now).await?;
        let changes = changes_of_copy(&snapshot.container, reader.today.as_deref());
        let source_id = snapshot.container.id.clone();
        let source_is_template = snapshot.container.is_template;
        let kind = snapshot.container.kind;

        if kind == ContainerKind::Wiki {
            let limits = target_app.limits().pages;
            let tree: PageTree = snapshot
                .blocks
                .iter()
                .filter(|b| b.block_type == BlockType::Page)
                .cloned()
                .map(PageNode::from)
                .collect();
            if tree.len() > usize::try_from(limits.pages).unwrap_or(usize::MAX) {
                return Err(AppError::from(BlockError::TooManyPages {
                    limit: limits.pages,
                }));
            }
            for block in snapshot
                .blocks
                .iter()
                .filter(|b| b.block_type == BlockType::Page)
            {
                if tree.depth(&block.id) > usize::try_from(limits.depth).unwrap_or(usize::MAX) {
                    return Err(refused_without_member(fvoci_store::StoreError::from(
                        BlockError::PageTooDeep {
                            limit: limits.depth,
                        },
                    )));
                }
            }
        }

        let prepared_container = build_copied_container(
            snapshot.container,
            target_ws_id,
            user_id,
            custom_title,
            &reader,
            changes,
            now,
        )?;
        let schedules = build_copied_schedules(snapshot.schedule, changes.days_later)?;

        let Renewed {
            containers_and_blocks:
                ContainersAndBlocks {
                    mut containers,
                    mut blocks,
                },
            renamed: all_copies,
        } = renew_containers_and_blocks_ids_told(
            ContainersAndBlocks {
                containers: vec![prepared_container],
                blocks: snapshot.blocks,
            },
            |_| true,
        );

        let new_container_id = containers
            .first()
            .map(|c| c.id.clone())
            .ok_or_else(|| AppError::internal("missing renewed container"))?;

        let referenced_files = collect_referenced_files(&blocks);
        let (file_map, staged_files, written_places) = target_app
            .pre_copy_files_to_container(
                source_app,
                &source_id,
                target_org_id,
                &new_container_id,
                &referenced_files,
                user_id,
            )
            .await?;
        let missing_files_count = referenced_files
            .iter()
            .filter(|fid| !file_map.contains_key(fid.as_str()))
            .count();

        let target_members = target_app.workspace_members(target_ws_id).await?;
        let mut present_users: HashSet<String> = target_members
            .into_iter()
            .filter(|m| m.role != WorkspaceRole::Guest)
            .map(|m| m.user_id)
            .collect();
        present_users.insert(user_id.to_owned());

        if let Err(error) = sanitize_copied_blocks(
            &mut blocks,
            &containers[0],
            &file_map,
            &all_copies,
            &snapshot.external_titles,
            &snapshot.member_names,
            &present_users,
            changes.days_later,
            reader.language,
            user_id,
            now,
        ) {
            release_written_places(written_places).await;
            return Err(error);
        }

        let ordered_blocks = order_parents_before_children(blocks, &new_container_id);
        let block_ids: Vec<String> = ordered_blocks.iter().map(|b| b.id.clone()).collect();
        let members = commit_copied_stream(
            target_app,
            &mut containers,
            &staged_files,
            &schedules,
            user_id,
            ordered_blocks,
            written_places,
        )
        .await?;

        let created_container = containers
            .into_iter()
            .next()
            .ok_or_else(|| AppError::internal("missing committed container"))?;

        target_app
            .add_containers_to_default_category(
                user_id,
                target_ws_id,
                std::slice::from_ref(&created_container.id),
            )
            .await?;

        target_app
            .metrics()
            .container_copied(ContainerWork::Duplicate, counted_as(kind));
        if kind == ContainerKind::Course {
            target_app.metrics().course_created(if source_is_template {
                CourseSource::Template
            } else {
                CourseSource::Duplicate
            });
        }

        announce_workspace_copy(
            target_app,
            target_ws_id,
            created_container.clone(),
            block_ids,
            members,
        );
        let _ = (snapshot.org_id, snapshot.org_kind);
        Ok(CopiedContainerResult {
            container: created_container,
            missing_files_count,
        })
    }
}

async fn read_source_course_schedule(
    source_app: &App,
    container: &Container,
) -> Result<Option<CourseScheduleUpdate>, AppError> {
    if container.kind != ContainerKind::Course {
        return Ok(None);
    }
    match source_app
        .store()
        .get_course_schedule(&container.id)
        .await?
    {
        Some(schedule) => Ok(Some(CourseScheduleUpdate {
            start_date: schedule.start_date,
            target_date: schedule.target_date,
            study_days: i32::from(schedule.study_days),
            auto_assign: false,
        })),
        None => {
            let Some(raw) = container.properties.get(TEMPLATE_SCHEDULE_PROPERTY) else {
                return Ok(None);
            };
            let mut parsed: CourseScheduleUpdate =
                serde_json::from_value(raw.clone()).map_err(|err| {
                    AppError::bad_request(format_args!("invalid course schedule: {err}"))
                })?;
            parsed.auto_assign = false;
            Ok(Some(parsed))
        }
    }
}

fn build_copied_schedules(
    schedule: Option<CourseScheduleUpdate>,
    days_later: i64,
) -> Result<Vec<(usize, ValidatedCourseSchedule)>, AppError> {
    let Some(mut schedule) = schedule else {
        return Ok(Vec::new());
    };
    if days_later != 0 {
        if let Some(shifted) = add_days(&schedule.start_date, days_later) {
            schedule.start_date = shifted;
        }
        if let Some(shifted) = add_days(&schedule.target_date, days_later) {
            schedule.target_date = shifted;
        }
    }
    let validated = schedule.check_request()?;
    Ok(vec![(0, validated)])
}

fn build_copied_container(
    mut container: Container,
    target_ws_id: &str,
    user_id: &str,
    custom_title: Option<&str>,
    reader: &Reader,
    changes: CopyChanges<'_>,
    now: i64,
) -> Result<Container, AppError> {
    let source_was_template = container.is_template;
    for mark in changes.without_properties {
        container.properties.remove(*mark);
    }
    for mark in TEMPLATE_MARKS {
        container.properties.remove(mark);
    }
    let title = match custom_title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(explicit) => explicit.to_owned(),
        None if source_was_template => container.title,
        None => (titles_in(reader.language, container.kind).copy_of)(&container.title),
    };
    container.workspace_id = target_ws_id.to_owned();
    container.visibility = ContainerVisibility::Private;
    container.title = title;
    container.is_template = false;
    container.is_builtin = false;
    container.template_version = 0;
    container.created_by = user_id.to_owned();
    container.modified_by = user_id.to_owned();
    container.created_at = now;
    container.updated_at = now;
    container.deleted_at = None;
    if container.id.is_empty() {
        container.id = fvoci_model::new_id();
    }
    container.validate().map_err(AppError::bad_request)?;
    Ok(container)
}

fn collect_referenced_files(blocks: &[Block]) -> Vec<String> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for block in blocks {
        for file_id in block.shown_files() {
            if !file_id.is_empty() && seen.insert(file_id.clone()) {
                files.push(file_id);
            }
        }
    }
    files
}

fn build_member_name_map(members: &[WorkspaceMember]) -> HashMap<String, String> {
    members
        .iter()
        .map(|m| {
            let label = if m.display_name.trim().is_empty() {
                m.username.clone()
            } else {
                m.display_name.clone()
            };
            (m.user_id.clone(), label)
        })
        .collect()
}

#[expect(
    clippy::too_many_arguments,
    reason = "single pass over copied blocks remaps files, references, mentions, dates, and people"
)]
fn sanitize_copied_blocks(
    blocks: &mut Vec<Block>,
    container: &Container,
    file_map: &HashMap<String, String>,
    all_copies: &HashMap<String, String>,
    external_titles: &HashMap<String, String>,
    member_names: &HashMap<String, String>,
    present_users: &HashSet<String>,
    days_later: i64,
    language: Language,
    user_id: &str,
    now: i64,
) -> Result<(), AppError> {
    let mut dropped_attachment_ids = HashSet::new();
    blocks.retain_mut(|block| {
        if block.block_type != BlockType::Attachment {
            return true;
        }
        let Some(old_file) = block.attachment_file().map(str::to_owned) else {
            dropped_attachment_ids.insert(block.id.clone());
            return false;
        };
        let Some(new_file) = file_map.get(&old_file) else {
            dropped_attachment_ids.insert(block.id.clone());
            return false;
        };
        block
            .fields
            .insert("fileId".to_owned(), Value::String(new_file.clone()));
        block.fields.remove("attachmentId");
        true
    });

    let date_props = date_property_ids(&container.card_properties);
    let untitled = match language {
        Language::Korean => "제목 없음",
        Language::English => "Untitled",
    };

    for block in blocks.iter_mut() {
        block.drop_malformed_fields();
        if !dropped_attachment_ids.is_empty()
            && let Some(Value::Array(order)) = block.fields.get_mut("contentOrder")
        {
            order.retain(|item| {
                item.as_str()
                    .is_none_or(|id| !dropped_attachment_ids.contains(id))
            });
        }
        if block.block_type == BlockType::Card {
            sanitize_card_properties(
                &mut block.fields,
                &container.card_properties,
                &date_props,
                present_users,
                days_later,
            );
            block.number = None;
        } else if block.block_type == BlockType::Unit && days_later != 0 {
            shift_copied_unit_date(&mut block.fields, days_later);
        } else if block.block_type == BlockType::View {
            sanitize_view_people(&mut block.fields, &container.card_properties, present_users);
        }

        if let Some(body) = block.body.take() {
            block.body = sanitize_copied_body(
                body,
                file_map,
                all_copies,
                external_titles,
                member_names,
                present_users,
                untitled,
            )?;
        }
        block.created_by = user_id.to_owned();
        block.modified_by = user_id.to_owned();
        block.created_at = now;
        block.updated_at = now;
        block.deleted_at = None;
        block.edited_at = 0;
        block.validate().map_err(AppError::from)?;
    }
    Ok(())
}

fn sanitize_card_properties(
    fields: &mut fvoci_model::JsonMap,
    definitions: &[CardPropertyDefinition],
    date_props: &HashSet<String>,
    present_users: &HashSet<String>,
    days_later: i64,
) {
    let Some(Value::Object(values)) = fields.get_mut("properties") else {
        return;
    };
    clear_values_of_another_kind(values, definitions);
    if days_later != 0 && !date_props.is_empty() {
        move_dates(values, date_props, days_later);
    }
    for def in definitions {
        match def.property_type {
            PropertyType::CreatedTime
            | PropertyType::UpdatedTime
            | PropertyType::CreatedBy
            | PropertyType::UpdatedBy => {
                values.remove(&def.id);
            }
            PropertyType::Person => {
                let keep = values
                    .get(&def.id)
                    .and_then(Value::as_str)
                    .is_some_and(|uid| present_users.contains(uid));
                if !keep {
                    values.remove(&def.id);
                }
            }
            PropertyType::MultiPerson => {
                if let Some(Value::Array(items)) = values.get_mut(&def.id) {
                    items.retain(|v| v.as_str().is_some_and(|uid| present_users.contains(uid)));
                    if items.is_empty() {
                        values.remove(&def.id);
                    }
                }
            }
            _ => {}
        }
    }
}

fn shift_copied_unit_date(fields: &mut fvoci_model::JsonMap, days_later: i64) {
    if let Some(day) = fields
        .get(fvoci_model::UNIT_SCHEDULED_DATE_FIELD)
        .and_then(Value::as_str)
        .and_then(|d| add_days(d, days_later))
    {
        fields.insert(
            fvoci_model::UNIT_SCHEDULED_DATE_FIELD.to_owned(),
            Value::String(day),
        );
    }
}

fn sanitize_view_people(
    fields: &mut fvoci_model::JsonMap,
    definitions: &[CardPropertyDefinition],
    present_users: &HashSet<String>,
) {
    let person_ids: HashSet<&str> = definitions
        .iter()
        .filter(|d| {
            matches!(
                d.property_type,
                PropertyType::Person
                    | PropertyType::MultiPerson
                    | PropertyType::CreatedBy
                    | PropertyType::UpdatedBy
            )
        })
        .map(|d| d.id.as_str())
        .collect();
    if person_ids.is_empty() {
        return;
    }
    if let Some(Value::Object(filter)) = fields.get_mut("filter")
        && let Some(Value::Array(filters)) = filter.get_mut("filters")
    {
        for entry in filters.iter_mut().filter_map(Value::as_object_mut) {
            let is_person = entry
                .get("propertyId")
                .and_then(Value::as_str)
                .is_some_and(|id| person_ids.contains(id));
            if is_person && let Some(Value::Array(vals)) = entry.get_mut("values") {
                vals.retain(|v| {
                    v.as_str()
                        .is_some_and(|uid| uid == "me" || present_users.contains(uid))
                });
            }
        }
    }
}

fn sanitize_copied_body(
    body: Body,
    file_map: &HashMap<String, String>,
    all_copies: &HashMap<String, String>,
    external_titles: &HashMap<String, String>,
    member_names: &HashMap<String, String>,
    present_users: &HashSet<String>,
    untitled: &str,
) -> Result<Option<Body>, AppError> {
    let Some(renamed) = body.with_files_renamed(|old| file_map.get(old).cloned()) else {
        return Ok(None);
    };
    let mut doc = renamed.document().clone();
    rewrite_body_nodes(
        &mut doc,
        all_copies,
        external_titles,
        member_names,
        present_users,
        untitled,
    );
    Body::parse(doc).map_err(|error| AppError::from_body_error("body", error))
}

fn rewrite_body_nodes(
    node: &mut Value,
    all_copies: &HashMap<String, String>,
    external_titles: &HashMap<String, String>,
    member_names: &HashMap<String, String>,
    present_users: &HashSet<String>,
    untitled: &str,
) {
    let kind = node.get("type").and_then(Value::as_str).unwrap_or("");
    if kind == "reference" {
        let target_id = node
            .get("attrs")
            .and_then(|a| a.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if let Some(new_id) = all_copies.get(target_id) {
            if let Some(attrs) = node.get_mut("attrs").and_then(Value::as_object_mut) {
                attrs.insert("id".to_owned(), Value::String(new_id.clone()));
            }
        } else {
            let raw_title = external_titles
                .get(target_id)
                .map(String::as_str)
                .unwrap_or("")
                .replace('\0', "");
            let label = if raw_title.trim().is_empty() {
                untitled
            } else {
                raw_title.as_str()
            };
            *node = json!({ "type": "text", "text": label });
        }
        return;
    }
    if kind == "mention" {
        let uid = node
            .get("attrs")
            .and_then(|a| a.get("userId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !present_users.contains(uid) {
            let name = member_names
                .get(uid)
                .map(String::as_str)
                .unwrap_or("user")
                .replace('\0', "");
            let clean = if name.trim().is_empty() {
                "user"
            } else {
                name.trim()
            };
            *node = json!({ "type": "text", "text": format!("@{clean}") });
        }
        return;
    }
    if let Some(children) = node.get_mut("content").and_then(Value::as_array_mut) {
        for child in children.iter_mut() {
            rewrite_body_nodes(
                child,
                all_copies,
                external_titles,
                member_names,
                present_users,
                untitled,
            );
        }
        coalesce_adjacent_plain_text(children);
    }
}

fn coalesce_adjacent_plain_text(children: &mut Vec<Value>) {
    let mut merged: Vec<Value> = Vec::with_capacity(children.len());
    for child in children.drain(..) {
        let plain_text = if child.get("type").and_then(Value::as_str) == Some("text")
            && child.get("marks").is_none()
        {
            child.get("text").and_then(Value::as_str).map(str::to_owned)
        } else {
            None
        };
        if let Some(next_str) = plain_text
            && let Some(prev) = merged.last_mut()
            && prev.get("type").and_then(Value::as_str) == Some("text")
            && prev.get("marks").is_none()
            && let Some(obj) = prev.as_object_mut()
            && let Some(Value::String(existing)) = obj.get_mut("text")
        {
            existing.push_str(&next_str);
            continue;
        }
        merged.push(child);
    }
    *children = merged;
}

fn order_parents_before_children(blocks: Vec<Block>, container_id: &str) -> Vec<Block> {
    let all_ids: HashSet<String> = blocks.iter().map(|b| b.id.clone()).collect();
    let mut emitted = HashSet::with_capacity(blocks.len());
    let mut ordered = Vec::with_capacity(blocks.len());
    let mut remaining = blocks;
    while !remaining.is_empty() {
        let before = remaining.len();
        let mut deferred = Vec::new();
        for block in remaining {
            let ready = block.parent_id.is_empty()
                || block.parent_id == container_id
                || !all_ids.contains(&block.parent_id)
                || emitted.contains(&block.parent_id);
            if ready {
                emitted.insert(block.id.clone());
                ordered.push(block);
            } else {
                deferred.push(block);
            }
        }
        if deferred.len() == before {
            ordered.extend(deferred);
            break;
        }
        remaining = deferred;
    }
    ordered
}

async fn commit_copied_stream(
    target_app: &App,
    containers: &mut [Container],
    staged_files: &[fvoci_model::FileRecord],
    schedules: &[(usize, ValidatedCourseSchedule)],
    user_id: &str,
    ordered_blocks: Vec<Block>,
    written_places: Vec<WrittenPlace>,
) -> Result<Vec<ContainerMember>, AppError> {
    let (tx, rx) = mpsc::channel(2);
    let producer = tokio::spawn(async move {
        for chunk in ordered_blocks.chunks(COPY_STREAM_BATCH) {
            if tx.send(StreamBatch::Batch(chunk.to_vec())).await.is_err() {
                return;
            }
        }
        let _ = tx.send(StreamBatch::Done).await;
    });

    let write_res = target_app
        .store()
        .create_imported_containers_blocks_files_members_and_schedules_streamed(
            containers,
            staged_files,
            &[],
            schedules,
            user_id,
            rx,
        )
        .await;
    let _ = producer.await;

    match write_res {
        Ok(members) => {
            for place in written_places {
                place.settled_in_transaction();
            }
            Ok(members)
        }
        Err(error) => {
            release_written_places(written_places).await;
            Err(match error {
                StreamCreateError::Producer(err) => err,
                StreamCreateError::Incomplete => {
                    AppError::internal("container copy stream ended before completion")
                }
                StreamCreateError::Store(err) => AppError::from(err),
            })
        }
    }
}

async fn release_written_places(places: Vec<WrittenPlace>) {
    for place in places {
        place.give_up().await;
    }
}

fn announce_workspace_copy(
    target_app: &App,
    target_ws_id: &str,
    container: Container,
    block_ids: Vec<String>,
    members: Vec<ContainerMember>,
) {
    let app = target_app.clone();
    let ws_id = target_ws_id.to_owned();
    target_app.spawn_background(async move {
        app.ws()
            .broadcast_container_change(&ws_id, &container)
            .await;
        for member in &members {
            app.ws()
                .broadcast_member_change(&ws_id, &member.container_id, member)
                .await;
        }
    });
    target_app.announce_blocks(target_ws_id, block_ids);
}
