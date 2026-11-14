//! Validation of upload, archive and document resource settings.

use super::{Configuration, MAX_UPLOAD_PART_BYTES, MIN_UPLOAD_PART_BYTES};
use anyhow::bail;

impl Configuration {
    /// Checks the limits on uploads: none turns a limit off, and they fit together.
    pub(super) fn check_upload_limits(&self) -> anyhow::Result<()> {
        // An S3 multipart upload takes no smaller part but the last, and none larger.
        if !(MIN_UPLOAD_PART_BYTES..=MAX_UPLOAD_PART_BYTES).contains(&self.upload_part_bytes) {
            bail!(
                "upload_part_bytes must be {MIN_UPLOAD_PART_BYTES} (5 MiB) to \
                 {MAX_UPLOAD_PART_BYTES} (5 GiB)"
            );
        }
        // Zero once meant "no limit"; a server without one is not something to end up
        // with by leaving a number at what it was (`docs/s6-design.md` 4.2).
        let largest = self
            .upload_part_bytes
            .saturating_mul(u64::try_from(fvoci_model::MAX_UPLOAD_PARTS).unwrap_or(0));
        if u64::try_from(self.max_file_size).map_or(true, |size| size == 0 || size > largest) {
            bail!(
                "maxfilesize must be 1 to {largest} bytes (upload_part_bytes times 10,000 \
                 parts), not {}",
                self.max_file_size
            );
        }
        // Less would refuse the largest file with 429 every time, as if it might fit later.
        // Positive, as checked above.
        let max_file_size = u64::try_from(self.max_file_size).unwrap_or(0);
        if self.max_pending_upload_bytes < max_file_size {
            bail!(
                "max_pending_upload_bytes must be at least maxfilesize ({max_file_size}), \
                 not {}",
                self.max_pending_upload_bytes
            );
        }
        // An archive comes as an upload in parts too.
        if self.max_import_bytes == 0 || self.max_import_bytes > largest {
            bail!(
                "max_import_bytes must be 1 to {largest} bytes (upload_part_bytes times \
                 10,000 parts), not {}",
                self.max_import_bytes
            );
        }
        if self.max_pending_upload_bytes < self.max_import_bytes {
            bail!(
                "max_pending_upload_bytes must be at least max_import_bytes ({}), not {}",
                self.max_import_bytes,
                self.max_pending_upload_bytes
            );
        }
        // Zero would keep not even the text a comment was posted with.
        if self.max_comment_versions == 0 {
            bail!("max_comment_versions must be at least 1");
        }
        // Zero would refuse every export or import.
        if self.max_concurrent_exports == 0 {
            bail!("max_concurrent_exports must be at least 1");
        }
        if self.max_concurrent_imports == 0 {
            bail!("max_concurrent_imports must be at least 1");
        }
        // Zero would take no reader of a public link at all: sharing is turned off for that.
        if self.public_link_connections_per_board == 0 {
            bail!("public_link_connections_per_board must be at least 1");
        }
        if self.public_link_connections_per_address == 0 {
            bail!("public_link_connections_per_address must be at least 1");
        }
        // Zero would give up every download before its first piece.
        if self.export_stall_seconds == 0 {
            bail!("export_stall_seconds must be at least 1");
        }
        self.collab.check()
    }
}
