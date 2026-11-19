SELECT set_config('fvoci.system', 'on', true);
-- (No row of an organization is read or changed here: the policy names the table it
-- is on, and a migration that names one begins in the system's scope.)
-- The files of the built-in templates are read by every organization, as their boards
-- and blocks are (0014_organizations.sql): the pictures and documents that the bodies
-- of the templates show are files of the system organization's boards, and a board made
-- from a template gets copies of its own. Reading only, and only by a transaction that
-- names an organization; the wall keeps everybody but the system organization from
-- changing them.
CREATE POLICY org_templates ON file_info FOR SELECT USING (
    org_id = '00000000-0000-7000-8000-00000000a000'
    AND (SELECT NULLIF(current_setting('fvoci.org', true), '')) IS NOT NULL
);
