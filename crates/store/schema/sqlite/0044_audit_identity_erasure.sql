-- Who an identity tied by a record is, is taken out of the records about a person
-- whose account is being erased, and nothing else of a record is ever changed: the
-- same as schema/postgres/0044_audit_identity_erasure.sql, which says what the guard
-- lets through. `IS` compares a NULL as a value.
DROP TRIGGER audit_log_unchanged;
CREATE TRIGGER audit_log_unchanged BEFORE UPDATE ON audit_log
WHEN NOT (
    NEW.identity IS NULL
    AND NEW.id IS OLD.id AND NEW.org_id IS OLD.org_id
    AND NEW.workspace_id IS OLD.workspace_id AND NEW.actor_id IS OLD.actor_id
    AND NEW.actor_kind IS OLD.actor_kind AND NEW.action IS OLD.action
    AND NEW.target_id IS OLD.target_id AND NEW.subject_id IS OLD.subject_id
    AND NEW.create_at IS OLD.create_at AND NEW.detail IS OLD.detail
    AND EXISTS (SELECT 1 FROM users
                WHERE id IN (OLD.actor_id, OLD.subject_id) AND erase_started_at > 0))
BEGIN
    SELECT RAISE(ABORT, 'a record of the audit log is not changed or removed');
END;
