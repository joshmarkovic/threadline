use super::*;
use crate::{
    Change,
    resources::{ResourceError, ensure_resource},
};
use crate::{
    ItemLocator, ItemUpdate,
    resources::{ResourcePage, ResourceProgress, ResourceRecord, Validators, commit_page_in},
};
use rusqlite::{Transaction, params};

/// None means the provider omitted this facet. An empty supplied list verifies zero matches.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListFacets {
    pub presentation: IssuePresentation,
    pub author: Option<String>,
    pub labels: Option<Vec<String>>,
    pub assignees: Option<Vec<String>>,
    pub reviewers: Option<Vec<String>>,
    pub participants: Option<Vec<String>>,
    pub mentions: Option<Vec<String>>,
}
pub struct ListSummary {
    pub number: u64,
    pub update: ItemUpdate,
    pub facets: ListFacets,
}
pub struct RepositoryListPage<'a> {
    pub summaries: Vec<ListSummary>,
    pub next_cursor: Option<&'a str>,
    pub completed_validators: Option<&'a Validators>,
    pub fetched_at_ms: i64,
}
fn apply_facets(tx: &Transaction<'_>, item: ItemId, at: i64, facets: ListFacets) -> Result<()> {
    super::metadata::save_presentation(tx, item, at, facets.presentation)?;
    for (category, values) in [
        ("author", facets.author.map(|s| vec![s])),
        ("label", facets.labels),
        ("assignee", facets.assignees),
        ("reviewer", facets.reviewers),
        ("participant", facets.participants),
        ("mention", facets.mentions),
    ] {
        let Some(values) = values else { continue };
        let changed = tx.execute("INSERT INTO reader_facets(item_id,category,observed_at) VALUES(?,?,?)
            ON CONFLICT(item_id,category) DO UPDATE SET observed_at=excluded.observed_at WHERE reader_facets.observed_at<excluded.observed_at", params![item, category, at])?;
        if changed == 0 {
            continue;
        }
        tx.execute(
            "DELETE FROM reader_facet_values WHERE item_id=? AND category=?",
            params![item, category],
        )?;
        let mut insert = tx.prepare_cached("INSERT INTO reader_facet_values(item_id,category,value) VALUES(?,?,?) ON CONFLICT DO NOTHING")?;
        for value in values {
            checked_label(&value)?;
            insert.execute(params![item, category, value])?;
        }
    }
    Ok(())
}
impl Store {
    /// Commit identities, independently observed facets, visible summaries and the page checkpoint together.
    pub fn commit_repository_list_page(
        &self,
        account: AccountId,
        repository: RepositoryId,
        expected: &ResourceProgress,
        page: RepositoryListPage<'_>,
    ) -> Result<Option<ResourceProgress>> {
        self.commit_list(account, repository, expected, page, None)
    }
    pub fn commit_repository_list_partial(
        &self,
        account: AccountId,
        repository: RepositoryId,
        expected: &ResourceProgress,
        page: RepositoryListPage<'_>,
        reason: ResourceError,
    ) -> Result<()> {
        self.commit_list(account, repository, expected, page, Some(reason))?;
        Ok(())
    }
    fn commit_list(
        &self,
        account: AccountId,
        repository: RepositoryId,
        expected: &ResourceProgress,
        page: RepositoryListPage<'_>,
        terminal_error: Option<ResourceError>,
    ) -> Result<Option<ResourceProgress>> {
        if page.summaries.len() > 100 {
            return Err(Error::InvalidInput(
                "Repository list page exceeds GitHub's page size",
            ));
        }
        let key = repository_list_key(account, repository);
        self.write(|tx| {
            let mut records = Vec::with_capacity(page.summaries.len());
            for summary in page.summaries {
                let at = summary.update.remote_updated_at.0;
                let id = identity::ingest_in(
                    tx,
                    account,
                    ItemLocator {
                        repository,
                        number: summary.number,
                    },
                    summary.update,
                )?;
                apply_facets(tx, id, at, summary.facets)?;
                records.push(ResourceRecord {
                    key: id.0.to_string(),
                    body: String::new(),
                });
            }
            commit_page_in(
                tx,
                key,
                expected,
                ResourcePage {
                    records: &records,
                    next_cursor: page.next_cursor,
                    completed_validators: page.completed_validators,
                    fetched_at_ms: page.fetched_at_ms,
                },
                terminal_error,
            )
        })
    }
}

impl Store {
    /// Publish directly fetched item metadata/body and their independent coverage without touching discussion or timeline state.
    pub fn commit_item_metadata(
        &self,
        account: AccountId,
        repository: RepositoryId,
        summary: ListSummary,
        validators: &Validators,
        fetched_at_ms: i64,
    ) -> Result<ItemId> {
        let updated = summary.update.remote_updated_at.0;
        let supplied_metadata = matches!(summary.update.title, Change::Value(_))
            && matches!(summary.update.closed, Change::Value(_));
        let supplied_body = summary.update.body != Change::Missing;
        self.write(|tx| {
            let item = identity::ingest_in(
                tx,
                account,
                ItemLocator {
                    repository,
                    number: summary.number,
                },
                summary.update,
            )?;
            apply_facets(tx, item, updated, summary.facets)?;
            let (title_at, closed_at, body_at): (Option<i64>, Option<i64>, Option<i64>) =
                tx.query_row("SELECT title_at,closed_at,body_at FROM items WHERE id=?", [item], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            for (family, supplied, latest) in [
                (
                    ResourceFamily::Metadata,
                    supplied_metadata,
                    title_at.max(closed_at),
                ),
                (ResourceFamily::Body, supplied_body, body_at),
            ] {
                if latest.is_some_and(|at| at > updated)
                    || family == ResourceFamily::Body && !supplied
                {
                    continue;
                }
                let id = ensure_resource(
                    tx,
                    ResourceKey {
                        account,
                        target: ResourceTarget::Item(item),
                        family,
                    },
                )?;
                if supplied {
                    tx.execute(
                        "UPDATE cached_resources SET coverage=2,fetch=0,error=NULL,last_page_at_ms=?2,last_fetch_at_ms=?2,last_validation_at_ms=?2,
                       etag=?3,last_modified=?4,revision=?5 WHERE id=?1 AND fetch!=1 AND (last_validation_at_ms IS NULL OR last_validation_at_ms<=?2)",
                        params![
                            id,
                            fetched_at_ms,
                            validators.etag,
                            validators.last_modified,
                            validators.revision
                        ],
                    )?;
                } else {
                    tx.execute("UPDATE cached_resources SET coverage=max(coverage,1),last_page_at_ms=?2 WHERE id=?1 AND fetch!=1", [id, fetched_at_ms])?;
                }
            }
            Ok(item)
        })
    }
}
