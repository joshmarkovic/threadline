use super::*;
use threadline_core::github::reader::{history::HistoryRefKind, refresh::ReadValidators};
use threadline_storage::{
    history::{HistoryEntry, HistoryRef, HistoryWalk},
    resources::CommitOid,
    sync::{MonitoredAncestors, MonitoredBranches},
};

pub(super) enum Claimed {
    Branches {
        rows: Vec<(String, CommitOid)>,
        next: Option<u32>,
        partial: bool,
    },
    Reference(Option<CommitOid>),
    Ancestors {
        walk: Box<HistoryWalk>,
        rows: Vec<HistoryEntry>,
        next: Option<String>,
        partial: bool,
    },
    Complete,
}

impl Claimed {
    pub(super) fn contacted(&self) -> bool {
        !matches!(self, Self::Complete)
    }

    pub(super) fn commit(
        self,
        store: &Store,
        claim: &ReadClaim,
        now: i64,
        zone: &str,
    ) -> threadline_storage::Result<bool> {
        match self {
            Self::Branches {
                rows,
                next,
                partial,
            } => store.commit_history_branches(
                claim,
                MonitoredBranches {
                    rows: &rows,
                    next,
                    partial,
                },
                now,
                zone,
            ),
            Self::Reference(tip) => store.commit_history_ref(claim, tip.as_ref(), now, zone),
            Self::Ancestors {
                walk,
                rows,
                next,
                partial,
            } => store.commit_history_ancestors(
                claim,
                MonitoredAncestors {
                    walk: Some(&walk),
                    rows: &rows,
                    next: next.as_deref(),
                    partial,
                },
                now,
                zone,
            ),
            Self::Complete => store.commit_history_ancestors(
                claim,
                MonitoredAncestors {
                    walk: None,
                    rows: &[],
                    next: None,
                    partial: false,
                },
                now,
                zone,
            ),
        }
    }
}

pub(super) async fn read(
    store: &Arc<Store>,
    reader: GitHubReader<'_>,
    claim: &ReadClaim,
    cancel: &Cancellation,
) -> Result<(Claimed, RateLimit), ReaderError> {
    let saved = claim.clone();
    let target = blocking(store, move |store| {
        store.monitored_history_target(&saved, UtcMillis::now()?.0)
    })
    .await?;
    match target {
        HistoryRead::Branches => {
            let page = crate::reader::history_page_number(claim.cursor.as_deref())?;
            let response = reader
                .history_refs(HistoryRefKind::Branch, page, cancel)
                .await?;
            let page = response.value.ok_or(ReaderError::InvalidIdentity)?;
            let rows = page
                .records
                .into_iter()
                .map(|row| Ok((row.name, CommitOid::parse(&row.oid)?)))
                .collect::<threadline_storage::Result<Vec<_>>>()?;
            Ok((
                Claimed::Branches {
                    rows,
                    next: page.next_page,
                    partial: page.limited,
                },
                response.rate,
            ))
        }
        HistoryRead::Reference(reference) => {
            let response = match reference {
                HistoryRef::Branch(name) => reader
                    .history_branch(&name, cancel)
                    .await
                    .map(|response| (response.value.map(|reference| reference.oid), response.rate)),
                HistoryRef::FollowedPr(number) => {
                    let account = claim.account;
                    let locator = threadline_storage::ItemLocator {
                        repository: claim.target.repository,
                        number: number.get(),
                    };
                    let expected = blocking(store, move |store| {
                        let item = match store.resolve(account, locator)? {
                            Some(item) => store.item(account, item)?,
                            None => None,
                        };
                        if item.as_ref().is_some_and(|item| item.locator != locator) {
                            return Err(Error::Conflict(
                                "The followed pull request changed repository",
                            ));
                        }
                        Ok(item.and_then(|item| item.pull_request.map(|details| details.id.get())))
                    })
                    .await?;
                    reader
                        .pr_snapshot(
                            number
                                .get()
                                .try_into()
                                .map_err(|_| ReaderError::InvalidIdentity)?,
                            &ReadValidators::default(),
                            cancel,
                        )
                        .await
                        .map(|response| {
                            let oid = response.value.and_then(|pull| {
                                let details = pull.details();
                                if expected.is_some_and(|id| id != details.pull_request_id.get()) {
                                    return None;
                                }
                                Some(details.head_oid.clone())
                            });
                            (oid, response.rate)
                        })
                }
                HistoryRef::Tag(_) => return Err(ReaderError::InvalidIdentity),
            };
            match response {
                Ok((oid, rate)) => {
                    let oid = oid.ok_or(ReaderError::InvalidIdentity)?;
                    Ok((Claimed::Reference(Some(CommitOid::parse(&oid)?)), rate))
                }
                Err(error)
                    if error.kind == AccessKind::MissingOrInaccessible
                        && error.http_status == Some(404) =>
                {
                    Ok((Claimed::Reference(None), error.rate.unwrap_or_default()))
                }
                Err(error) => Err(error.into()),
            }
        }
        HistoryRead::Ancestors(_) => {
            let saved = claim.clone();
            let walk = blocking(store, move |store| {
                store.prepare_history_ancestors(&saved, UtcMillis::now()?.0)
            })
            .await?;
            let Some(walk) = walk else {
                return Ok((Claimed::Complete, RateLimit::default()));
            };
            let page = crate::reader::history_page_number(walk.cursor.as_deref())?;
            let response = reader
                .history_commits(walk.tip.as_str(), page, cancel)
                .await?;
            let page = response.value.ok_or(ReaderError::InvalidIdentity)?;
            let rows = page
                .records
                .into_iter()
                .map(crate::reader::history_entry)
                .collect::<Result<Vec<_>, _>>()?;
            let next = if page.limited {
                None
            } else {
                page.next_page.map(|page| page.to_string())
            };
            Ok((
                Claimed::Ancestors {
                    walk: Box::new(walk),
                    rows,
                    next,
                    partial: page.limited,
                },
                response.rate,
            ))
        }
    }
}
