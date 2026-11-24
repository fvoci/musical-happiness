-- Subscriptions belong to users alone: drop `subscriber_type` (after removing any
-- non-user rows) and rename `subscriber_id` to `user_id`, both in `subscriptions` and
-- in the saved `watchedBy` arrays of `deleted_blocks`.

DELETE FROM subscriptions WHERE subscriber_type <> 'user';

DROP INDEX subscriptions_subscriber_id_idx;
ALTER TABLE subscriptions DROP COLUMN subscriber_type;
ALTER TABLE subscriptions RENAME COLUMN subscriber_id TO user_id;
CREATE INDEX subscriptions_user_id_idx ON subscriptions (user_id);

UPDATE deleted_blocks
SET block = (
    SELECT CASE
        WHEN json_array_length(kept.watchers) = 0
            THEN json_remove(deleted_blocks.block, '$.watchedBy')
        ELSE json_set(deleted_blocks.block, '$.watchedBy', json(kept.watchers))
    END
    FROM (
        SELECT json_group_array(
            json_set(
                json_remove(w.value, '$.subscriberType', '$.subscriberId'),
                '$.userId',
                json_extract(w.value, '$.subscriberId')
            )
            ORDER BY w.key
        ) AS watchers
        FROM json_each(deleted_blocks.block, '$.watchedBy') AS w
        WHERE json_extract(w.value, '$.subscriberType') = 'user'
    ) AS kept
)
WHERE json_type(deleted_blocks.block, '$.watchedBy') = 'array';
