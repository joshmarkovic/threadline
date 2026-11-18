use threadline_core::github::reader::pr as remote;
use threadline_storage::pr::{InlineComment, PatchEvidence, ReviewAnchor};

use super::*;

pub(super) fn validators(values: ReadValidators) -> Validators {
    Validators {
        etag: values.etag,
        last_modified: values.last_modified,
        revision: None,
    }
}

pub(super) fn page<T>(
    data: remote::PrPage<T>,
    validators: Validators,
    read: impl Fn(T) -> PrRecord,
) -> FetchPage {
    let next = data.next_page.map(|page| page.to_string());
    receipt(
        data.records.into_iter().map(read).collect(),
        next,
        data.partial,
        validators,
        None,
    )
}

pub(super) fn files(data: remote::PrFiles, validators: Validators) -> FetchPage {
    receipt(
        data.records.into_iter().map(file).collect(),
        None,
        data.partial,
        validators,
        None,
    )
}

pub(super) fn graph<T>(data: remote::PrGraphPage<T>, read: impl Fn(T) -> PrRecord) -> FetchPage {
    receipt(
        data.records.into_iter().map(read).collect(),
        data.next_cursor,
        data.partial,
        Validators::default(),
        data.review_decision,
    )
}

fn receipt(
    records: Vec<PrRecord>,
    next: Option<String>,
    incomplete: bool,
    validators: Validators,
    review_decision: Option<String>,
) -> FetchPage {
    FetchPage {
        records,
        next: if incomplete { None } else { next },
        partial: incomplete.then_some(ResourceError::InvalidResponse),
        validators,
        merge_base: None,
        review_decision,
    }
}

pub(super) fn commit(value: remote::PrCommit) -> PrRecord {
    PrRecord::Commit {
        oid: value.oid,
        message: value.message,
        author: value.author,
        authored_at_ms: value.authored_at_ms,
        committed_at_ms: value.committed_at_ms,
    }
}

pub(in crate::reader) fn file(value: remote::PrFile) -> PrRecord {
    let patch = match value.patch {
        remote::PatchEvidence::Text(value) => PatchEvidence::Text(value),
        remote::PatchEvidence::Truncated(value) => PatchEvidence::Truncated(value),
        remote::PatchEvidence::Binary => PatchEvidence::Binary,
        remote::PatchEvidence::Unavailable => PatchEvidence::Unavailable,
    };
    PrRecord::File {
        path: value.path,
        previous_path: value.previous_path,
        status: value.status,
        additions: value.additions,
        deletions: value.deletions,
        patch,
    }
}

pub(super) fn check(value: remote::PrCheck) -> PrRecord {
    PrRecord::Check {
        id: value.id,
        head_oid: value.head_oid,
        name: value.name,
        status: value.status,
        conclusion: value.conclusion,
        completed_at_ms: value.completed_at_ms,
    }
}

pub(super) fn status(value: remote::PrStatus) -> PrRecord {
    PrRecord::Status {
        id: value.id,
        head_oid: value.head_oid,
        context: value.context,
        state: value.state,
        updated_at_ms: value.updated_at_ms,
    }
}

pub(super) fn review(value: remote::PrReview) -> PrRecord {
    PrRecord::Review {
        id: value.id,
        node_id: Some(value.node_id),
        author: value.author,
        actor: value.actor,
        body: value.body,
        state: value.state,
        submitted_at_ms: value.submitted_at_ms,
        commit_oid: value.commit_oid,
    }
}

pub(super) fn thread(value: remote::PrThread) -> PrRecord {
    PrRecord::Thread {
        node_id: value.node_id().to_owned(),
        resolved: value.resolved,
        outdated: value.outdated,
        comments: value.comments.into_iter().map(inline_comment).collect(),
        replies_complete: value.replies_complete,
        reply_cursor: value.reply_cursor,
    }
}

pub(super) fn inline_comment(value: remote::PrInlineComment) -> InlineComment {
    InlineComment {
        id: value.id,
        node_id: Some(value.node_id),
        in_reply_to: value.in_reply_to,
        author: value.author,
        actor: value.actor,
        body: value.body,
        created_at_ms: Some(value.created_at_ms),
        updated_at_ms: Some(value.updated_at_ms),
        anchor: value.anchor.map(|anchor| ReviewAnchor {
            path: anchor.path,
            line: anchor.line,
            original_oid: anchor.original_oid,
            side: match anchor.side {
                PrSide::Left => SourceSide::Left,
                PrSide::Right => SourceSide::Right,
            },
        }),
    }
}
