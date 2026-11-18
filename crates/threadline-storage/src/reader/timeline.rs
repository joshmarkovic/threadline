use super::*;
use crate::resources::{
    ResourcePage, ResourceProgress, ResourceRecord, ResourceState, Validators, state_in,
};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_item: Option<LinkedItem>,
    pub remote_key: String,
    pub event: String,
    pub actor: Option<String>,
    pub occurred_at_ms: Option<i64>,
}
pub struct TimelinePage {
    pub entries: Vec<TimelineEntry>,
    pub next: Option<String>,
    pub state: ResourceState,
}
pub struct TimelineInputPage<'a> {
    pub events: Vec<TimelineEntry>,
    pub next_cursor: Option<&'a str>,
    pub completed_validators: Option<&'a Validators>,
    pub fetched_at_ms: i64,
}
impl Store {
    pub fn commit_timeline_page(
        &self,
        account: AccountId,
        item: ItemId,
        expected: &ResourceProgress,
        page: TimelineInputPage<'_>,
    ) -> Result<Option<ResourceProgress>> {
        if page.events.len() > 100 {
            return Err(Error::InvalidInput(
                "Timeline page exceeds GitHub's page size",
            ));
        }
        let key = ResourceKey {
            account,
            target: ResourceTarget::Item(item),
            family: ResourceFamily::Timeline,
        };
        let mut records = Vec::with_capacity(page.events.len());
        for event in page.events {
            checked_label(&event.remote_key)?;
            checked_label(&event.event)?;
            if let Some(link) = &event.linked_item {
                link.validate()?;
            }
            let body = serde_json::to_string(&event)
                .map_err(|_| Error::InvalidInput("Invalid timeline entry"))?;
            records.push(ResourceRecord {
                key: event.remote_key,
                body,
            });
        }
        self.commit_resource_page(
            key,
            expected,
            ResourcePage {
                records: &records,
                next_cursor: page.next_cursor,
                completed_validators: page.completed_validators,
                fetched_at_ms: page.fetched_at_ms,
            },
        )
    }
    pub fn timeline_page(
        &self,
        account: AccountId,
        item: ItemId,
        after: Option<&str>,
        limit: u16,
    ) -> Result<TimelinePage> {
        limit_ok(limit)?;
        self.read(|conn| {
            let db = conn.unchecked_transaction()?;
            let key = ResourceKey {
                account,
                target: ResourceTarget::Item(item),
                family: ResourceFamily::Timeline,
            };
            let state = state_in(&db, key)?;
            let mut query = db.prepare_cached("WITH events AS (
                  SELECT r.record_key,r.body,json_extract(r.body,'$.occurred_at_ms') AS at FROM cached_resources c
                  JOIN cached_resource_records r ON r.resource_id=c.id WHERE c.account_id=?1 AND c.item_id=?2 AND c.family='timeline')
                SELECT record_key,body FROM events WHERE ?3 IS NULL OR (at IS NULL,coalesce(at,0),record_key)>
                    (SELECT at IS NULL,coalesce(at,0),record_key FROM events WHERE record_key=?3)
                ORDER BY at IS NULL,at,record_key LIMIT ?4")?;
            let mut records = query
                .query_map(params![account, item, after, i64::from(limit) + 1], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let next = if records.len() > usize::from(limit) {
                records.pop();
                records.last().map(|(k, _)| k.clone())
            } else {
                None
            };
            let entries = records
                .into_iter()
                .map(|(_, raw)| serde_json::from_str(&raw).map_err(|_| Error::Corrupt))
                .collect::<Result<_>>()?;
            Ok(TimelinePage {
                entries,
                next,
                state,
            })
        })
    }
}

/// A GitHub timeline source route. A number alone cannot identify a linked issue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LinkedItem {
    pub owner: String,
    pub repository: String,
    pub number: u64,
    pub pull_request: bool,
    pub provider_repository_id: Option<u64>,
    pub provider_issue_id: Option<u64>,
}
impl LinkedItem {
    fn validate(&self) -> Result<i64> {
        threadline_core::domain::RepositoryName::new(&self.owner, &self.repository)
            .map_err(|_| Error::InvalidInput("Invalid GitHub timeline source repository"))?;
        if self.provider_repository_id == Some(0) || self.provider_issue_id == Some(0) {
            return Err(Error::InvalidInput("Invalid linked provider identity"));
        }
        i64::try_from(self.number)
            .ok()
            .filter(|n| *n > 0)
            .ok_or(Error::InvalidInput("Invalid GitHub timeline source number"))
    }
}
impl Store {
    /// Open only a cached identity corroborated by the source route, kind and immutable issue ID.
    pub fn resolve_linked_item(
        &self,
        account: AccountId,
        link: &LinkedItem,
    ) -> Result<Option<ItemId>> {
        let number = link.validate()?;
        let Some(id) = link.provider_issue_id else {
            return Ok(None);
        };
        let kind = if link.pull_request {
            "pull_request"
        } else {
            "issue"
        };
        self.read(|db| Ok(db.query_row(
            "SELECT i.id FROM items i JOIN repositories r ON r.id=i.repository_id AND r.account_id=i.account_id
             WHERE i.account_id=?1 AND i.number=?2 AND i.kind=?3 AND i.issue_api_id=?4
               AND r.owner=?5 AND r.name=?6 AND (?7 IS NULL OR r.provider_repository_id=?7)",
            params![account,number,kind,id.to_string(),link.owner,link.repository,link.provider_repository_id.map(|v|v.to_string())],
            |row|row.get(0)).optional()?))
    }
}
