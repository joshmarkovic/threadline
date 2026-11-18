use std::fmt::Write;

use gpui_kit::component::diff::DiffFile;

#[cfg(feature = "e2e")]
pub(crate) const REVISION: &str = "s01-feasibility-v1";
pub(crate) const COPY_SENTENCE: &str = "Offline review keeps cached changes close.";
pub(crate) const DISCUSSION_LEAD_BLOCK: usize = 3;
pub(crate) const PATCH_PATH: &str = "src/cache.rs";
pub(crate) const ISSUE_TITLE: &str = "Keep the last review available offline";
pub(crate) const PRIMARY_PR: &str = "Keep cached reads available after startup";
pub(crate) const SECONDARY_PR: &str = "Explain why an older revision is visible";

pub(crate) const PR_BODIES: [&str; 2] = [
    "Resolves #41.\n\nKeep the cached review on screen while an incremental refresh runs. If a refresh fails, keep the visible revision and show its age.\n\n**Review focus**\n\nCheck that cancellation leaves the cached revision intact.",
    "Resolves #42.\n\nShow the last successful refresh timestamp without interrupting a review.\n\n**Review focus**\n\nCheck the difference between stale data and a disconnected session.",
];

pub(crate) const LINKED_BODIES: [&str; 2] = [
    "A reviewer should be able to finish reading a downloaded discussion after disconnecting. The cache must retain the last confirmed revision.",
    "Reviewers need to know when the last refresh succeeded. Show its age near the cached revision, with details available on demand.",
];

pub(crate) fn issue_discussion() -> String {
    let mut body = format!(
        "**Open issue · #41**\n\n## {ISSUE_TITLE}\n\n*Mira Chen · 96 replies*\n\n{COPY_SENTENCE}\n\n"
    );
    body.push_str(concat!(
        "A review can start while connected and finish on a train without losing its place. ",
        "Keep the last confirmed revision available while a refresh runs in the background.\n\n",
        "### What should happen\n\n",
        "- [x] Leave the existing discussion visible during background work.\n",
        "- [ ] Show the age of a cached revision beside the refresh control.\n",
        "- [ ] Keep drafts in the pane that owns them.\n\n",
        "> Downloaded changes remain readable when transport is interrupted.\n\n",
        "```rust\nlet revision = cache.last_confirmed(item);\n```\n\n",
        "### Discussion\n\n",
    ));
    let comments = [
        "Keep the code and its thread attached to the same revision. An older review can still be useful if we label it clearly.",
        "Load the cached conversation first. An unsuccessful refresh should stay scoped to the part of the conversation it was checking.",
        "After a reconnect, let the reviewer finish selecting code before swapping in newly available content.",
        "Keyboard focus must stay with the editor. A background status change should be readable without interrupting typing.",
    ];
    for (index, comment) in comments.iter().cycle().take(96).enumerate() {
        let author = ["Mira Chen", "Sam Ortega", "Asha Reed", "Nico Ali"][index % 4];
        writeln!(body, "**{author} · Comment {}**\n\n{comment}\n", index + 1)
            .expect("write into String");
    }
    body
}

pub(crate) fn patches(seed: u64, large: bool) -> [Vec<DiffFile>; 2] {
    [
        DiffFile::parse(&patch(seed, if large { 10_000 } else { 2_000 }))
            .expect("deterministic cache patch"),
        DiffFile::parse(concat!(
            "--- a/src/status.rs\n",
            "+++ b/src/status.rs\n",
            "@@ -1,4 +1,4 @@\n",
            " fn label(age: Duration) -> String {\n",
            "-    format!(\"{age:?}\")\n",
            "+    format!(\"Last refreshed {age:?} ago\")\n",
            " }\n",
            " // Only shown for cached data.\n",
        ))
        .expect("deterministic status patch"),
    ]
}

fn patch(seed: u64, rows: usize) -> String {
    let mut patch = format!(
        concat!(
            "diff --git a/{path} b/{path}\n",
            "--- a/{path}\n",
            "+++ b/{path}\n",
            "@@ -1,{rows} +1,{rows} @@\n",
            " impl CacheReader {{\n",
            "     pub fn lookup(&self, item: ItemKey) -> Option<Revision> {{\n",
            "-        self.live.get(&item).cloned()\n",
            "+        self.visible.get(&item).cloned()\n",
            "     }}\n",
        ),
        path = PATCH_PATH,
        rows = rows,
    );
    for index in 4..rows - 1 {
        let suffix = (index as u64).wrapping_add(seed) % 97;
        if index % 5 == 0 {
            writeln!(
                patch,
                "-    fn load_{index}() {{ read_remote({suffix}); }}\n+    fn load_{index}() {{ read_cached({suffix}); }}"
            )
            .expect("write into String");
        } else {
            writeln!(patch, "     // Keep revision slot {index} intact.")
                .expect("write into String");
        }
    }
    patch.push_str(" }\n");
    patch.push_str(concat!(
        "diff --git a/docs/cache.md b/docs/cache.md\n",
        "--- a/docs/cache.md\n",
        "+++ b/docs/cache.md\n",
        "@@ -1,2 +1,3 @@\n",
        " # Cached reviews\n",
        "-Wait for an online check before reading.\n",
        "+Open the latest downloaded review.\n",
        "+A refresh can run in the background.\n",
    ));
    patch
}

pub(crate) struct Commit {
    pub lane: usize,
    pub parents: &'static [usize],
    pub sha: &'static str,
    pub title: &'static str,
    pub author: &'static str,
    pub branch: Option<&'static str>,
}

pub(crate) const HISTORY: [Commit; 5] = [
    Commit {
        lane: 0,
        parents: &[1, 2],
        sha: "2e31acd",
        title: "Merge the cached review changes",
        author: "Mira Chen",
        branch: Some("main"),
    },
    Commit {
        lane: 1,
        parents: &[3],
        sha: "9bdce47",
        title: "Retain the visible revision",
        author: "Asha Reed",
        branch: Some("offline-review"),
    },
    Commit {
        lane: 0,
        parents: &[3],
        sha: "eac7185",
        title: "Improve stale-state labels",
        author: "Sam Ortega",
        branch: None,
    },
    Commit {
        lane: 0,
        parents: &[4],
        sha: "7743fab",
        title: "Index the latest confirmed revision",
        author: "Nico Ali",
        branch: None,
    },
    Commit {
        lane: 0,
        parents: &[],
        sha: "642ca82",
        title: "Add the cache reader",
        author: "Mira Chen",
        branch: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_diff_fixture_has_reviewed_files_and_large_hunks() {
        let [normal, status] = patches(42, false);
        let [large, _] = patches(42, true);
        assert_eq!(normal.len(), 2);
        assert_eq!(normal[0].path().as_ref(), PATCH_PATH);
        assert_eq!(normal[0].additions(), 400);
        assert_eq!(normal[0].deletions(), 400);
        assert_eq!(normal[1].path().as_ref(), "docs/cache.md");
        assert_eq!(status[0].path().as_ref(), "src/status.rs");
        assert_eq!(large[0].additions(), 2_000);
    }
}
