SELECT set_config('fvoci.system', 'on', true);
-- (No row of an organization is read or changed here: the guard's body names the
-- tables it asks, and a migration that names them begins in the system's scope.)
-- A record of the audit log is never changed, but for one thing: who an identity it
-- tied is (`identity`, an email address or a subject, 0035) is taken out of the
-- records about a person whose account is being erased. The record stays, with who
-- did what to whom by their identifiers, which are all an erased account keeps. The
-- guard lets through an update that sets `identity` to NULL, on a record that names
-- (as who did it, or whom it was done to) an account whose erasing began, and that
-- leaves every other column as it is; anything else it refuses as before.
CREATE OR REPLACE FUNCTION audit_log_is_kept() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND (
           NOT EXISTS (SELECT 1 FROM organizations WHERE id = OLD.org_id)
           OR (OLD.workspace_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM workspaces WHERE id = OLD.workspace_id))) THEN
        RETURN OLD;
    END IF;
    IF TG_OP = 'UPDATE'
       AND NEW.identity IS NULL
       AND (NEW.id, NEW.org_id, NEW.workspace_id, NEW.actor_id, NEW.actor_kind, NEW.action,
            NEW.target_id, NEW.subject_id, NEW.create_at, NEW.detail)
           IS NOT DISTINCT FROM
           (OLD.id, OLD.org_id, OLD.workspace_id, OLD.actor_id, OLD.actor_kind, OLD.action,
            OLD.target_id, OLD.subject_id, OLD.create_at, OLD.detail)
       AND EXISTS (SELECT 1 FROM users
                   WHERE id IN (OLD.actor_id, OLD.subject_id) AND erase_started_at > 0) THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'a record of the audit log is not changed or removed'
        USING ERRCODE = 'restrict_violation';
END $$;
