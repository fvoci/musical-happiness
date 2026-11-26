//! Boards and their members.

use fvoci_model::{
    AuditAction, Container, ContainerCategoryWebsocketData, ContainerKind, ContainerMember,
    ContainerPatch, ContainerVisibility, new_id,
};
use fvoci_store::{AuditEntry, DeletedContainer, Joined, MemberChange, PublicContainers, Standing};

use super::{App, ContainerAccess, DEFAULT_CATEGORY_CONTAINERS, Former};
use crate::collab::{AccessScope, Deletion};
use crate::error::{AppError, Reason};
use crate::metrics::CourseSource;
use crate::permissions::ContainerScope;

/// Whether a patch of a board changes who may do what on it: its visibility (open to the
/// workspace or by invitation) or the role of who joins it.
pub(super) fn changes_access(patch: &ContainerPatch) -> bool {
    patch.visibility.is_some() || patch.minimum_role.is_some()
}

/// The message of the refusal to remove or demote a board's only admin.
const LAST_ADMIN: &str = "cannot leave a board with no admins";

/// Whether the user who creates a board becomes a member of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CreatorMembership {
    /// The creator becomes an admin and editor of the board.
    Admin,
    /// The board starts without members.
    None,
}

impl App {
    /// Returns a board: for a handle of one workspace ([`App::in_workspace`]), only a
    /// board of that workspace or a built-in template.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, or the store's error.
    pub async fn container(&self, container_id: &str) -> Result<Container, AppError> {
        match self.store().get_container(container_id).await {
            // A board of another workspace than the handle's is not there.
            Ok(container) if self.reaches_container(&container) => Ok(container),
            Ok(_) => Err(AppError::not_found("board")),
            Err(error) if error.is_not_found() => Err(AppError::not_found("board")),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns the boards of a workspace that a user is a member of and, if asked for, the
    /// workspace's open boards: of the kinds the handle reaches ([`App::reaching`]).
    /// Templates are left out.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn containers_for_user_and_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
        public_containers: PublicContainers,
    ) -> Result<Vec<Container>, AppError> {
        Ok(self
            .store()
            .get_containers_for_user_and_workspace(
                user_id,
                workspace_id,
                public_containers,
                self.kinds(),
            )
            .await?)
    }

    /// Creates a board without members and files it under the default category of
    /// `user_id`, unless it is a template. The board gets a new identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the board already has an identifier, if it cannot be stored,
    /// or if it cannot be filed under the user's default category; in the last case the
    /// board exists nonetheless.
    pub async fn create_container(
        &self,
        container: Container,
        user_id: &str,
    ) -> Result<Container, AppError> {
        self.create_container_as(container, user_id, CreatorMembership::None)
            .await
    }

    /// Creates a board as [`App::create_container`] does and makes `user_id` an admin and
    /// editor of it.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::create_container`].
    pub async fn create_container_with_admin(
        &self,
        container: Container,
        user_id: &str,
    ) -> Result<Container, AppError> {
        self.create_container_as(container, user_id, CreatorMembership::Admin)
            .await
    }

    async fn create_container_as(
        &self,
        mut container: Container,
        user_id: &str,
        membership: CreatorMembership,
    ) -> Result<Container, AppError> {
        if !container.id.is_empty() {
            return Err(AppError::bad_request("new board cannot have an ID").with_field("id"));
        }
        self.require_makes(&container)?;
        container.id = new_id();

        let creator = match membership {
            CreatorMembership::Admin => Some(
                self.store()
                    .insert_container_with_admin(&mut container, user_id)
                    .await?,
            ),
            CreatorMembership::None => {
                self.store()
                    .insert_container(&mut container, user_id)
                    .await?;
                None
            }
        };

        let app = self.clone();
        let created = container.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_container_change(&created.workspace_id, &created)
                .await;
            if let Some(creator) = creator {
                app.ws()
                    .broadcast_member_change(&created.workspace_id, &created.id, &creator)
                    .await;
            }
        });

        if !container.is_template {
            self.add_containers_to_default_category(
                user_id,
                &container.workspace_id,
                std::slice::from_ref(&container.id),
            )
            .await?;
            if container.kind == ContainerKind::Course {
                self.metrics().course_created(CourseSource::Blank);
            }
        }
        Ok(container)
    }

    /// Files boards under a user's default category in a workspace, creating that category
    /// if the user does not have it yet.
    pub(super) async fn add_containers_to_default_category(
        &self,
        user_id: &str,
        workspace_id: &str,
        container_ids: &[String],
    ) -> Result<(), AppError> {
        let categories = self.user_category_containers(user_id, workspace_id).await?;
        let Some(default) = categories
            .iter()
            .find(|entry| entry.category.name == DEFAULT_CATEGORY_CONTAINERS)
        else {
            return Err(AppError::internal(format_args!(
                "no default category found for user userID: {user_id}"
            )));
        };
        self.add_update_user_category_container(
            workspace_id,
            user_id,
            &default.category.id,
            container_ids,
        )
        .await
    }

    /// Applies a patch to a board on behalf of a user and tells the board's members.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, or the store's error.
    pub async fn patch_container(
        &self,
        patch: &ContainerPatch,
        container_id: &str,
        user_id: &str,
    ) -> Result<Container, AppError> {
        let mut is_template = false;
        let mut scope_before = None;
        if patch.visibility.is_some() {
            let container = match self.store().get_container(container_id).await {
                Ok(container) => container,
                Err(error) if error.is_not_found() => {
                    return Err(AppError::not_found("board"));
                }
                Err(error) => return Err(error.into()),
            };
            is_template = container.is_template;
            scope_before = Some(ContainerScope::of(&container));
        }

        if changes_access(patch) {
            self.collab()
                .narrowing_access(self, &AccessScope::container(container_id));
        }
        let updated = self
            .store()
            .patch_container(container_id, patch, user_id)
            .await?;
        // Who read an open wiki or course without joining it reads it no more once it is
        // private: they are told it is gone for them, as when it is deleted.
        let closed = scope_before.filter(|before| {
            *before == ContainerScope::OpenToRead
                && ContainerScope::of(&updated) != ContainerScope::OpenToRead
        });
        if changes_access(patch) {
            self.collab()
                .recheck_access(self, AccessScope::container(container_id))
                .await;
        }

        let app = self.clone();
        let container = updated.clone();
        let new_visibility = patch.visibility.filter(|_| is_template);
        self.spawn_background(async move {
            app.ws()
                .broadcast_container_change(&container.workspace_id, &container)
                .await;
            if let Some(visibility) = new_visibility {
                app.broadcast_workspace_users(&container, visibility).await;
            }
            if let Some(scope) = closed {
                app.broadcast_closed(&container, scope).await;
            }
        });

        Ok(updated)
    }

    /// Puts the card properties of a board into the order of their identifiers in
    /// `order`, which names each of them once, tells the board's listeners with the
    /// board's change as for any change of it, and returns the order.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, a bad request if `order`
    /// does not name each property once, and the store's error.
    pub async fn reorder_card_properties(
        &self,
        container_id: &str,
        order: &[String],
        user_id: &str,
    ) -> Result<Vec<String>, AppError> {
        let container = match self
            .store()
            .reorder_card_properties(container_id, order, user_id)
            .await
        {
            Ok(container) => container,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("board")),
            Err(error) => return Err(error.into()),
        };
        let app = self.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_container_change(&container.workspace_id, &container)
                .await;
        });
        Ok(order.to_vec())
    }

    /// Tells who read the board as it was, `scope`, and no longer does that it is gone
    /// for them: the message of its deletion, to who reads it no more alone. Nothing is
    /// told if it is open again by now: a later change made it so, and told so.
    async fn broadcast_closed(&self, container: &Container, scope: ContainerScope) {
        let now = match self.store().get_container(&container.id).await {
            Ok(now) => Some(ContainerScope::of(&now)),
            Err(error) if error.is_not_found() => None,
            Err(error) => {
                tracing::error!(%error, "Unable to read the board made private");
                return;
            }
        };
        if now == Some(scope) {
            return;
        }
        let members = match self.effective_container_members(&container.id).await {
            Ok(members) => members,
            Err(error) => {
                tracing::error!(%error, "Unable to get the board members");
                return;
            }
        };
        self.ws()
            .broadcast_closed_to_others(&container.workspace_id, &container.id, &members, scope)
            .await;
    }

    /// Tells who works in the workspace and is no member of a template that they gained or
    /// lost access to it, after the template changed between open and private.
    async fn broadcast_workspace_users(
        &self,
        container: &Container,
        visibility: ContainerVisibility,
    ) {
        // A team's people keep their role across the change, as direct members do.
        let members = match self.effective_container_members(&container.id).await {
            Ok(members) => members,
            Err(error) => {
                tracing::error!(%error, "Unable to get the board members");
                Vec::new()
            }
        };
        // Who works in the board's workspace, whom an open template is for: nobody else
        // gains or loses it, and nobody else is named.
        let users = match self
            .store()
            .get_workspace_worker_ids(&container.workspace_id)
            .await
        {
            Ok(users) => users,
            Err(error) => {
                tracing::error!(%error, "Unable to get the workspace users");
                Vec::new()
            }
        };
        let others = users
            .iter()
            .filter(|user_id| !members.iter().any(|member| member.user_id == **user_id));
        for user_id in others {
            match visibility {
                ContainerVisibility::Private => {
                    self.ws()
                        .broadcast_member_delete(&container.workspace_id, &container.id, user_id)
                        .await;
                }
                ContainerVisibility::Open => {
                    let viewer = ContainerMember {
                        user_id: user_id.clone(),
                        container_id: container.id.clone(),
                        role: fvoci_model::ContainerRole::Viewer,
                        synthetic: true,
                        inherited_role: None,
                    };
                    self.ws()
                        .broadcast_member_change(&container.workspace_id, &container.id, &viewer)
                        .await;
                }
            }
        }
    }

    /// Deletes a board with its blocks and tells the board's members. Deleting a board
    /// that does not exist succeeds.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the board cannot be read or deleted.
    pub async fn delete_container(
        &self,
        container_id: &str,
        user_id: &str,
    ) -> Result<(), AppError> {
        let container = match self.store().get_container(container_id).await {
            Ok(container) => container,
            Err(error) if error.is_not_found() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        // The memberships go with the board; who is told is who was a member.
        let members = self
            .store()
            .effective_container_members(container_id)
            .await?;
        let (removed, by) = (container_id.to_owned(), user_id.to_owned());
        self.collab()
            .remove(
                self,
                Deletion::Containers(vec![removed.clone()]),
                async move |app| Ok(app.store().delete_container(&removed, &by).await?),
            )
            .await?;
        self.collab()
            .recheck_access(self, AccessScope::container(container_id))
            .await;

        let former = Former {
            members: &members,
            scope: ContainerScope::of(&container),
        };
        self.ws()
            .broadcast_container_delete(&container.workspace_id, &container.id, former)
            .await;
        // The board's sharing went with it; so does whoever followed it by its link.
        self.ws().end_readers_of_container(&container.id);
        Ok(())
    }

    /// Returns a board that is in the trash with the memberships it had, or `None` if
    /// no board of that identifier is there.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the trash cannot be read.
    pub async fn deleted_container(
        &self,
        container_id: &str,
    ) -> Result<Option<DeletedContainer>, AppError> {
        match self.store().get_deleted_container(container_id).await {
            Ok(deleted) => Ok(Some(deleted)),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Restores a board from the trash, with its blocks, its file records, the
    /// memberships and places in the sidebar of the users who still exist and the roles
    /// of its surviving teams, rechecks its card rooms, tells the board's members and
    /// each user it is filed for, and returns the board.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the board is not in the trash, and the store's
    /// error if it cannot be restored.
    pub async fn undelete_container(
        &self,
        container_id: &str,
        modified_by: &str,
    ) -> Result<Container, AppError> {
        let restored = {
            let _structure = self.collab().changing_structure(self, container_id).await;
            self.store()
                .undelete_container(container_id, modified_by)
                .await
        };
        let container = match restored {
            Ok(container) => container,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("board")),
            Err(error) => return Err(error.into()),
        };
        // Its people and its surviving teams have their roles on it again. Its rooms
        // were emptied when it was deleted: this is for an entry under way, which
        // reads the roles again.
        self.collab()
            .recheck_access(self, AccessScope::container(&container.id))
            .await;

        let app = self.clone();
        let restored = container.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_container_change(&restored.workspace_id, &restored)
                .await;
            app.tell_where_restored_container_is(&restored).await;
        });
        Ok(container)
    }

    /// Tells each user who has a restored board filed where it is: the board's entry, and
    /// the order of the boards of that category, which moved to make room for it. Read
    /// after the restore; a failed read is logged, the restore stands.
    async fn tell_where_restored_container_is(&self, container: &Container) {
        let places = match self.store().get_container_places(&container.id).await {
            Ok(places) => places,
            Err(error) => {
                tracing::error!(
                    %error,
                    container_id = container.id,
                    "the places of a restored board could not be read"
                );
                return;
            }
        };
        for place in places {
            let filed = ContainerCategoryWebsocketData {
                container_id: container.id.clone(),
                category_id: place.category_id.clone(),
                hidden: place.hidden,
            };
            self.ws()
                .broadcast_category_container_change(
                    &container.workspace_id,
                    &place.user_id,
                    std::slice::from_ref(&filed),
                )
                .await;
            let categories = self
                .store()
                .get_user_category_containers(&place.user_id, &container.workspace_id)
                .await;
            let order: Option<Vec<String>> = categories.ok().and_then(|categories| {
                categories
                    .into_iter()
                    .find(|entry| entry.category.id == place.category_id)
                    .map(|entry| {
                        entry
                            .container_metadata
                            .into_iter()
                            .map(|metadata| metadata.container_id)
                            .collect()
                    })
            });
            if let Some(order) = order {
                self.ws()
                    .broadcast_category_containers_reorder(
                        &container.workspace_id,
                        &place.user_id,
                        &place.category_id,
                        &order,
                    )
                    .await;
            }
        }
    }

    /// Returns the explicit members of a board.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn members_for_container(
        &self,
        container_id: &str,
    ) -> Result<Vec<ContainerMember>, AppError> {
        Ok(self.store().get_members_for_container(container_id).await?)
    }

    /// Returns the members of a board who are in its workspace now, as the board's member
    /// list shows them: who left the workspace is listed nowhere, whatever role is left.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the members cannot be read.
    pub async fn present_members_for_container(
        &self,
        container_id: &str,
    ) -> Result<Vec<ContainerMember>, AppError> {
        Ok(self
            .store()
            .get_present_members_for_container(container_id)
            .await?)
    }

    /// Makes a user a member of a board with the roles of `member`, files the board
    /// under the user's default category unless it is a template, and tells the board's
    /// members.
    ///
    /// A user who is a member already stays as they are, and that membership is
    /// returned. `None` is returned if the board does not exist.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read or written, or an
    /// error if the board cannot be filed under the user's default category; in that
    /// case the membership exists nonetheless.
    pub async fn add_member_to_container(
        &self,
        member: &ContainerMember,
    ) -> Result<Option<ContainerMember>, AppError> {
        self.add_member_to_container_recorded(member, Word::Unasked)
            .await
    }

    /// Makes a user a member of a board as [`App::add_member_to_container`] does, on the
    /// word of who asked, whose permission was checked with `access`: only if they are
    /// still what it found when the membership is written (`Store::add_member_asked`).
    ///
    /// # Errors
    ///
    /// As [`App::add_member_to_container`]; `None` also if who asked is not what they were.
    pub async fn add_member_asked(
        &self,
        member: &ContainerMember,
        user_id: &str,
        access: &ContainerAccess,
    ) -> Result<Option<ContainerMember>, AppError> {
        let word = Word::Asked(standing(user_id, access));
        self.add_member_to_container_recorded(member, word).await
    }

    /// Makes a user a member of a board, on behalf of `actor_id`, who manages the board
    /// without reading it, as [`App::add_member_to_container`] does and, if the membership
    /// is new, records in the organization's audit log in the same step who made whom a
    /// member: themselves, by which they come to read it, or somebody else, who might be
    /// another account of theirs.
    ///
    /// # Errors
    ///
    /// As [`App::add_member_to_container`]; a record that cannot be written is the store's
    /// error, and the membership is not made then.
    pub async fn add_member_as_manager(
        &self,
        member: &ContainerMember,
        actor_id: &str,
    ) -> Result<Option<ContainerMember>, AppError> {
        self.add_member_to_container_recorded(member, Word::Manager(actor_id))
            .await
    }

    /// Adds a member as [`App::add_member_to_container`] does and, if the membership is new
    /// and a manager is given, records the manager doing it in the audit log in the same
    /// step.
    async fn add_member_to_container_recorded(
        &self,
        member: &ContainerMember,
        word: Word<'_>,
    ) -> Result<Option<ContainerMember>, AppError> {
        let container = match self.store().get_container(&member.container_id).await {
            Ok(container) => container,
            Err(error) if error.is_not_found() => return Ok(None),
            Err(error) => return Err(error.into()),
        };

        // Added unless already there, in one step: of two requests that add the same
        // user at once, the second finds the membership the first made.
        let added = match word {
            Word::Manager(manager_id) => {
                let entry = audit_entry(&container, member, manager_id);
                self.store().add_member_recorded(member, &entry).await
            }
            Word::Asked(asked) => self.store().add_member_asked(member, asked).await,
            Word::Unasked => self.store().add_member(member).await,
        };
        let existing = match added {
            Ok(existing) => existing,
            // The board, or who asked as they were, is not there any more.
            Err(error) if error.is_not_found() && matches!(word, Word::Asked(_)) => {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        if let Some(existing) = existing {
            return Ok(Some(existing));
        }

        self.collab()
            .recheck_access(
                self,
                AccessScope::container(&container.id).of_user(&member.user_id),
            )
            .await;
        // The membership is stored: it is announced whatever becomes of the filing.
        self.broadcast_member_change_in_background(container.workspace_id.clone(), member.clone());
        if !container.is_template {
            self.add_containers_to_default_category(
                &member.user_id,
                &container.workspace_id,
                std::slice::from_ref(&container.id),
            )
            .await?;
        }
        Ok(Some(member.clone()))
    }

    /// Makes a user a member of a board with the board's minimum role, as joining does,
    /// and returns the membership. A member below the minimum role is raised to it; a
    /// member with that role or a higher one stays as they are. The board's members are
    /// told about a membership that is new or changed.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, and an error if the board
    /// cannot be filed under the user's default category; in that case the membership
    /// exists nonetheless.
    pub async fn join_container(
        &self,
        container_id: &str,
        user_id: &str,
    ) -> Result<ContainerMember, AppError> {
        let container = self.container(container_id).await?;
        let joined = match self.store().join_container(container_id, user_id).await {
            Ok(joined) => joined,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("board")),
            Err(error) => return Err(error.into()),
        };
        match joined {
            Joined::New(member) => {
                self.collab()
                    .recheck_access(self, AccessScope::container(&container.id).of_user(user_id))
                    .await;
                // The membership is stored: it is announced whatever becomes of the
                // filing.
                self.broadcast_member_change_in_background(
                    container.workspace_id.clone(),
                    member.clone(),
                );
                if !container.is_template {
                    self.add_containers_to_default_category(
                        user_id,
                        &container.workspace_id,
                        std::slice::from_ref(&container.id),
                    )
                    .await?;
                }
                Ok(member)
            }
            Joined::Raised(member) => {
                self.collab()
                    .recheck_access(self, AccessScope::container(&container.id).of_user(user_id))
                    .await;
                self.broadcast_member_change_in_background(container.workspace_id, member.clone());
                Ok(member)
            }
            Joined::Already(member) => Ok(member),
        }
    }

    /// Replaces the roles of a member of a board and tells the board's members.
    ///
    /// `None` is returned, and nothing is changed, if the board does not exist or the
    /// user is not a member of it.
    ///
    /// # Errors
    ///
    /// Returns a bad request if the change would leave the board without an admin, or
    /// the store's error.
    pub async fn update_container_member(
        &self,
        member: &ContainerMember,
    ) -> Result<Option<ContainerMember>, AppError> {
        self.update_container_member_recorded(member, Word::Unasked)
            .await
    }

    /// Replaces the roles of a member of a board as [`App::update_container_member`] does,
    /// on the word of who asked, as [`App::add_member_asked`] takes it.
    ///
    /// # Errors
    ///
    /// As [`App::update_container_member`]; `None` also if who asked is not what they were.
    pub async fn update_member_asked(
        &self,
        member: &ContainerMember,
        user_id: &str,
        access: &ContainerAccess,
    ) -> Result<Option<ContainerMember>, AppError> {
        let word = Word::Asked(standing(user_id, access));
        self.update_container_member_recorded(member, word).await
    }

    /// Gives a member of a board roles on it, on behalf of `actor_id`, who manages the
    /// board without reading it, as [`App::update_container_member`] does and records in the
    /// organization's audit log in the same step who gave whom the roles, as
    /// [`App::add_member_as_manager`] does.
    ///
    /// # Errors
    ///
    /// As [`App::update_container_member`]; a record that cannot be written is the store's
    /// error, and the membership is not changed then.
    pub async fn update_member_as_manager(
        &self,
        member: &ContainerMember,
        actor_id: &str,
    ) -> Result<Option<ContainerMember>, AppError> {
        self.update_container_member_recorded(member, Word::Manager(actor_id))
            .await
    }

    async fn update_container_member_recorded(
        &self,
        member: &ContainerMember,
        word: Word<'_>,
    ) -> Result<Option<ContainerMember>, AppError> {
        let container = match self.store().get_container(&member.container_id).await {
            Ok(container) => container,
            Err(error) if error.is_not_found() => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let scope = AccessScope::container(&container.id).of_user(&member.user_id);
        self.collab().narrowing_access(self, &scope);
        // A board keeps at least one admin. Whether this change would take the last one
        // away is decided where the change is written, so that two admins who step down
        // at once do not both pass.
        let changed = match word {
            Word::Manager(manager_id) => {
                let entry = audit_entry(&container, member, manager_id);
                self.store().change_member_recorded(member, &entry).await
            }
            Word::Asked(asked) => self.store().change_member_asked(member, asked).await,
            Word::Unasked => self.store().change_member(member).await,
        };
        let change = match changed {
            Ok(change) => change,
            // The board, or who asked as they were, is not there any more.
            Err(error) if error.is_not_found() && matches!(word, Word::Asked(_)) => {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        match change {
            MemberChange::Done => {}
            MemberChange::NotAMember => return Ok(None),
            MemberChange::LastAdmin => {
                return Err(AppError::conflict(LAST_ADMIN).with_reason(Reason::LastAdmin));
            }
        }

        self.collab()
            .recheck_access(
                self,
                AccessScope::container(&container.id).of_user(&member.user_id),
            )
            .await;
        self.broadcast_member_change_in_background(container.workspace_id, member.clone());
        Ok(Some(member.clone()))
    }

    pub(super) fn broadcast_member_change_in_background(
        &self,
        workspace_id: String,
        member: ContainerMember,
    ) {
        let app = self.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_member_change(&workspace_id, &member.container_id, &member)
                .await;
        });
    }

    /// Removes a user from a board and tells the board's members and the user. Nothing
    /// happens if the board does not exist or the user is not a member of it.
    ///
    /// # Errors
    ///
    /// Returns a bad request if the user is the board's only admin, or the store's
    /// error.
    pub async fn delete_container_member(
        &self,
        container_id: &str,
        user_id: &str,
    ) -> Result<(), AppError> {
        let container = match self.store().get_container(container_id).await {
            Ok(container) => container,
            Err(error) if error.is_not_found() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let scope = AccessScope::container(container_id).of_user(user_id);
        self.collab().narrowing_access(self, &scope);
        // A board keeps at least one admin; decided where the membership is removed.
        match self.store().remove_member(container_id, user_id).await? {
            MemberChange::Done => {}
            MemberChange::NotAMember => return Ok(()),
            MemberChange::LastAdmin => {
                return Err(AppError::conflict(LAST_ADMIN).with_reason(Reason::LastAdmin));
            }
        }

        self.collab()
            .recheck_access(self, AccessScope::container(container_id).of_user(user_id))
            .await;
        let app = self.clone();
        let user_id = user_id.to_owned();
        self.spawn_background(async move {
            app.ws()
                .broadcast_member_delete(&container.workspace_id, &container.id, &user_id)
                .await;
        });
        Ok(())
    }
}

/// On whose word a board's membership changes, and what is checked of them where it is
/// written.
#[derive(Clone, Copy)]
enum Word<'a> {
    /// Nobody's that is to be checked again: the server's own (an import).
    Unasked,
    /// A user's whose permission was checked: they must still be what it found.
    Asked(Standing<'a>),
    /// A workspace's administrator's who manages the board without reading it: they must
    /// still administer the workspace, and are recorded.
    Manager(&'a str),
}

/// What `access` found of the user who asks.
fn standing<'a>(user_id: &'a str, access: &'a ContainerAccess) -> Standing<'a> {
    Standing {
        user_id,
        workspace_role: access.actor.workspace,
        member: access.member.as_ref(),
        team_role: access.team_role,
    }
}

/// The record of a manager of the board, who does not read it, giving `member` its
/// roles: themselves, or somebody else.
fn audit_entry(container: &Container, member: &ContainerMember, manager_id: &str) -> AuditEntry {
    if member.user_id == manager_id {
        AuditEntry::new(
            &container.workspace_id,
            manager_id,
            AuditAction::SelfAddedToContainer,
            &container.id,
        )
    } else {
        AuditEntry::new(
            &container.workspace_id,
            manager_id,
            AuditAction::AddedToContainer,
            &container.id,
        )
        .with_subject(&member.user_id)
    }
}
