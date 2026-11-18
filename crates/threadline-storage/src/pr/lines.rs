use super::*;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceLine {
    /// One-based physical line in the stored unified patch, including hunk headers.
    pub patch_line: u32,
    pub left: Option<u32>,
    pub right: Option<u32>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    pub oid: CommitOid,
    pub path: String,
    pub line: u32,
    pub side: SourceSide,
}

static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@").expect("static Git hunk grammar")
});

#[derive(Default)]
struct Side {
    line: u32,
    remaining: u32,
}
impl Side {
    fn take(&mut self) -> Option<u32> {
        if self.remaining == 0 || self.line == 0 {
            return None;
        }
        let line = self.line;
        self.line = self.line.checked_add(1)?;
        self.remaining -= 1;
        Some(line)
    }
}

fn parse_hunks(patch: &str) -> (Vec<SourceLine>, bool) {
    let mut rows = Vec::new();
    let (mut left, mut right) = (Side::default(), Side::default());
    let mut hunk = false;
    for (index, text) in patch.lines().enumerate() {
        if text.starts_with("@@") {
            if left.remaining != 0 || right.remaining != 0 {
                return (rows, false);
            }
            let Some(c) = HEADER.captures(text) else {
                return (rows, false);
            };
            let number = |i| c.get(i).map_or(Some(1), |m| m.as_str().parse::<u32>().ok());
            let (Some(l), Some(lc), Some(r), Some(rc)) =
                (number(1), number(2), number(3), number(4))
            else {
                return (rows, false);
            };
            left = Side {
                line: l,
                remaining: lc,
            };
            right = Side {
                line: r,
                remaining: rc,
            };
            hunk = true;
            continue;
        }
        if !hunk || text.starts_with("\\ No newline at end of file") {
            continue;
        }
        let Ok(patch_line) = u32::try_from(index + 1) else {
            return (rows, false);
        };
        let entry = match text.as_bytes().first() {
            Some(b'+') => right.take().map(|r| SourceLine {
                patch_line,
                left: None,
                right: Some(r),
            }),
            Some(b'-') => left.take().map(|l| SourceLine {
                patch_line,
                left: Some(l),
                right: None,
            }),
            Some(b' ') => left.take().zip(right.take()).map(|(l, r)| SourceLine {
                patch_line,
                left: Some(l),
                right: Some(r),
            }),
            _ => None,
        };
        let Some(entry) = entry else {
            return (rows, false);
        };
        rows.push(entry);
    }
    (rows, hunk && left.remaining == 0 && right.remaining == 0)
}

pub(crate) fn file_mappings(record: &mut PrRecord) -> Vec<SourceLine> {
    let PrRecord::File {
        patch,
        additions,
        deletions,
        ..
    } = record
    else {
        return vec![];
    };
    match patch {
        PatchEvidence::Text(text) => {
            if text.is_empty() && *additions == Some(0) && *deletions == Some(0) {
                return Vec::new();
            }
            let (rows, complete) = parse_hunks(text);
            if !complete {
                *patch = PatchEvidence::Truncated(std::mem::take(text));
            }
            rows
        }
        PatchEvidence::Truncated(text) => parse_hunks(text).0,
        _ => vec![],
    }
}

impl Store {
    pub fn pr_source_line(
        &self,
        rev: &PrRevision,
        path: &str,
        patch_line: u32,
        side: SourceSide,
    ) -> Result<Option<SourceLocation>> {
        self.read(|db| {
            let Some(rid) = family_id(db, rev, PrFamily::Files)? else {
                return Ok(None);
            };
            let raw: Option<String> = db
                .query_row(
                    "SELECT body FROM cached_resource_records WHERE resource_id=? AND record_key=?",
                    params![rid, path],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(raw) = raw else { return Ok(None) };
            let row: StoredRecord = decoded(&raw)?;
            let PrRecord::File {
                path,
                previous_path,
                ..
            } = row.record
            else {
                return Err(Error::Corrupt);
            };
            let Some(mapping) = row
                .mappings
                .iter()
                .find(|line| line.patch_line == patch_line)
            else {
                return Ok(None);
            };
            let result = match side {
                SourceSide::Left => {
                    mapping
                        .left
                        .zip(rev.merge_base.clone())
                        .map(|(line, oid)| SourceLocation {
                            oid,
                            path: previous_path.unwrap_or(path),
                            line,
                            side,
                        })
                }
                SourceSide::Right => mapping.right.map(|line| SourceLocation {
                    oid: rev.head.clone(),
                    path,
                    line,
                    side,
                }),
            };
            Ok(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ut_011_source_coordinates_use_immutable_hunk_ranges_instead_of_display_positions() {
        let (rows, complete) =
            parse_hunks("@@ -8,2 +12,2 @@\n earlier\n-old\n+new\n@@ -100,0 +110,1 @@\n+later");
        assert!(complete);
        assert_eq!((rows[1].left, rows[1].right), (Some(9), None));
        assert_eq!((rows[3].left, rows[3].right), (None, Some(110)));
        assert!(!parse_hunks("@@ -2,4 +2,4 @@\n short").1);
        assert!(!parse_hunks("@@@ ambiguous").1);
    }
    #[test]
    fn ut_011_unicode_whitespace_and_line_endings_preserve_each_source_range() {
        for line_ending in ["\n", "\r\n"] {
            for removed in 0..5_u32 {
                for added in 0..5_u32 {
                    let mut lines = vec![format!("@@ -100,{} +40,{} @@", removed + 1, added + 1)];
                    let mut expected = vec![];
                    for old in 0..removed {
                        lines.push("-\t café  ".into());
                        expected.push((Some(100 + old), None));
                    }
                    for new in 0..added {
                        lines.push("+  日本語".into());
                        expected.push((None, Some(40 + new)));
                    }
                    lines.push(" closing line".into());
                    expected.push((Some(100 + removed), Some(40 + added)));
                    lines.push("\\ No newline at end of file".into());
                    let (mapped, complete) = parse_hunks(&lines.join(line_ending));
                    assert!(complete);
                    assert_eq!(
                        mapped
                            .iter()
                            .map(|line| (line.left, line.right))
                            .collect::<Vec<_>>(),
                        expected
                    );
                }
            }
        }
    }
}
