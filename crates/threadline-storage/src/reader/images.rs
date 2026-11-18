use super::*;

/// The image decoder validates dimensions and format before handing bytes to this disk cache.
pub struct CachedImage {
    pub mime: String,
    pub bytes: Vec<u8>,
    pub stored_at_ms: i64,
}
const IMAGE_LIMIT: usize = 8 * 1024 * 1024;
const ACCOUNT_LIMIT: i64 = 64 * 1024 * 1024;
fn image_key(url: &str) -> Result<()> {
    let parsed = url.strip_prefix("https://").and_then(|s| s.split_once('/'));
    if !parsed.is_some_and(|(host, path)| {
        !host.is_empty()
            && !path.is_empty()
            && host
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || matches!(v, b'.' | b'-'))
    }) || url.len() > 4096
        || url.contains(['?', '#', '\\'])
        || !url.bytes().all(|v| v.is_ascii_graphic())
    {
        return Err(Error::InvalidInput(
            "Image cache requires a stable HTTPS URL without credentials, query or fragment",
        ));
    }
    Ok(())
}
impl Store {
    /// Retain at most 64 MiB per account, evicting the oldest stored images first.
    pub fn cache_image(
        &self,
        account: AccountId,
        repository: RepositoryId,
        stable_url: &str,
        mime: &str,
        bytes: &[u8],
        at_ms: i64,
    ) -> Result<()> {
        image_key(stable_url)?;
        if bytes.is_empty()
            || bytes.len() > IMAGE_LIMIT
            || !matches!(
                mime,
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            )
        {
            return Err(Error::InvalidInput(
                "Unsupported image format or cache size",
            ));
        }
        self.write(|tx| {
            scope_check(tx, account, ReaderScope::Repository(repository))?;
            tx.execute(
                "INSERT INTO cached_images(account_id,repository_id,stable_url,mime,bytes,stored_at) VALUES(?,?,?,?,?,?)
                ON CONFLICT(account_id,repository_id,stable_url) DO UPDATE SET mime=excluded.mime,bytes=excluded.bytes,stored_at=excluded.stored_at
                WHERE cached_images.stored_at<=excluded.stored_at",
                params![account, repository, stable_url, mime, bytes, at_ms],
            )?;
            let pinned_bytes:i64=tx.query_row(
                "SELECT COALESCE(SUM(length(c.bytes)),0) FROM cached_images c JOIN pr_pinned_images p USING(account_id,repository_id,stable_url)
                 WHERE c.account_id=?",[account],|row|row.get(0),
            )?;
            if pinned_bytes>ACCOUNT_LIMIT { return Err(Error::Conflict("Pinned image bytes exceed the account cache limit")); }
            tx.execute("DELETE FROM cached_images WHERE account_id=?1 AND (repository_id,stable_url) IN (
                SELECT repository_id,stable_url FROM (SELECT repository_id,stable_url,
                sum(length(bytes)) OVER(ORDER BY EXISTS(SELECT 1 FROM pr_pinned_images p WHERE p.account_id=cached_images.account_id AND p.repository_id=cached_images.repository_id AND p.stable_url=cached_images.stable_url) DESC,stored_at DESC,repository_id,stable_url ROWS UNBOUNDED PRECEDING) AS retained
                FROM cached_images WHERE account_id=?1) WHERE retained>?2)", params![account, ACCOUNT_LIMIT])?;
            Ok(())
        })
    }
    pub fn cached_image(
        &self,
        account: AccountId,
        repository: RepositoryId,
        stable_url: &str,
    ) -> Result<Option<CachedImage>> {
        image_key(stable_url)?;
        self.read(|db| {
            scope_check(db, account, ReaderScope::Repository(repository))?;
            Ok(db
                .query_row("SELECT mime,bytes,stored_at FROM cached_images WHERE account_id=? AND repository_id=? AND stable_url=?", params![account, repository, stable_url], |r| {
                    Ok(CachedImage {
                        mime: r.get(0)?,
                        bytes: r.get(1)?,
                        stored_at_ms: r.get(2)?,
                    })
                })
                .optional()?)
        })
    }
}
