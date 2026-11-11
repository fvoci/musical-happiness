-- Where the file of an upload goes in the object store, chosen when the upload is
-- opened: an S3 multipart upload is made at its final key, part by part. Empty where
-- the parts are kept on the local disk and the place is chosen at completion.
ALTER TABLE uploads ADD COLUMN path TEXT NOT NULL DEFAULT '';
