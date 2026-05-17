use std::path::Path;

use tracing::{debug, warn};

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Hardlink,
    Reflink,
    Copy,
}

/// Link or copy one file, preferring zero-copy filesystem mechanisms.
///
/// The destination is removed first if it exists. Symlink handling remains the
/// caller's responsibility; this helper operates on regular files only.
pub async fn link_or_copy(src: &Path, dst: &Path) -> Result<LinkKind> {
    if let Some(parent) = dst.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if dst.exists() || dst.symlink_metadata().is_ok() {
        tokio::fs::remove_file(dst).await.ok();
    }

    match tokio::fs::hard_link(src, dst).await {
        Ok(()) => {
            debug!(src = %src.display(), dst = %dst.display(), "created hardlink");
            return Ok(LinkKind::Hardlink);
        }
        Err(e) if crate::fs::is_cross_device_error(&e) => {
            debug!(src = %src.display(), dst = %dst.display(), "hardlink crossed filesystem");
        }
        Err(e) => {
            debug!(src = %src.display(), dst = %dst.display(), "hardlink failed: {e}");
        }
    }

    let src = src.to_path_buf();
    let dst = dst.to_path_buf();
    match tokio::task::spawn_blocking({
        let src = src.clone();
        let dst = dst.clone();
        move || reflink_copy::reflink(&src, &dst)
    })
    .await
    .map_err(|e| CoreError::Other(format!("reflink task panicked: {e}").into()))?
    {
        Ok(()) => {
            debug!(src = %src.display(), dst = %dst.display(), "created reflink");
            return Ok(LinkKind::Reflink);
        }
        Err(e) => {
            warn!(
                src = %src.display(),
                dst = %dst.display(),
                "reflink failed; falling back to copy: {e}"
            );
            if dst.exists() || dst.symlink_metadata().is_ok() {
                tokio::fs::remove_file(&dst).await.ok();
            }
        }
    }

    tokio::fs::copy(&src, &dst).await.map_err(|e| {
        CoreError::Other(
            format!(
                "copy fallback failed: {} -> {}: {e}",
                src.display(),
                dst.display()
            )
            .into(),
        )
    })?;
    debug!(src = %src.display(), dst = %dst.display(), "copied file");
    Ok(LinkKind::Copy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn link_or_copy_replaces_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("src.bin");
        let dst = temp.path().join("nested").join("dst.bin");
        tokio::fs::create_dir_all(dst.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&src, b"source").await.unwrap();
        tokio::fs::write(&dst, b"old").await.unwrap();

        let kind = link_or_copy(&src, &dst).await.unwrap();

        assert!(matches!(
            kind,
            LinkKind::Hardlink | LinkKind::Reflink | LinkKind::Copy
        ));
        assert_eq!(tokio::fs::read(&dst).await.unwrap(), b"source");
    }
}
