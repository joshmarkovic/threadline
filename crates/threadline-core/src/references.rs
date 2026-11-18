//! Closing-style references are candidates for reading context. Their presence does not establish a server-side relationship.
use crate::domain::{ItemNumber, RepositoryName};
use markdown::mdast::Node;
use regex::Regex;
use std::{collections::BTreeSet, sync::LazyLock};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosingReference {
    pub repository: RepositoryName,
    pub number: ItemNumber,
    pub keyword: String,
    pub raw: String,
}

static CLOSING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(close[sd]?|fix(?:es|ed)?|resolve[sd]?)\b\s*:?\s*([^\s\x00]+)")
        .expect("static closing-reference grammar")
});

pub fn closing_references(body: &str, source: &RepositoryName) -> Vec<ClosingReference> {
    let Ok(root) = markdown::to_mdast(body, &markdown::ParseOptions::gfm()) else {
        return vec![];
    };
    let mut runs = Vec::new();
    collect_blocks(&root, &mut runs);
    let mut seen = BTreeSet::new();
    let mut references = Vec::new();
    for text in runs {
        for captures in CLOSING.captures_iter(&text) {
            let raw = captures[2].trim_end_matches([',', '.', ';', ':', '!', ')', ']']);
            let Some((repo, number)) = target(source, raw) else {
                continue;
            };
            let identity = (
                repo.owner().to_ascii_lowercase(),
                repo.name().to_ascii_lowercase(),
                number.get(),
            );
            if seen.insert(identity) {
                references.push(ClosingReference {
                    repository: repo,
                    number,
                    keyword: captures[1].to_owned(),
                    raw: raw.to_owned(),
                });
            }
        }
    }
    references
}

fn collect_blocks(node: &Node, runs: &mut Vec<String>) {
    if matches!(
        node,
        Node::Paragraph(_) | Node::Heading(_) | Node::TableCell(_)
    ) {
        let mut text = String::new();
        collect_inline(node, &mut text);
        runs.push(text);
    } else if let Some(children) = node.children() {
        for child in children {
            collect_blocks(child, runs);
        }
    }
}

fn collect_inline(node: &Node, text: &mut String) {
    match node {
        Node::Text(value) => text.push_str(&value.value),
        Node::Break(_) => text.push(' '),
        Node::InlineCode(_) | Node::InlineMath(_) | Node::Html(_) => text.push('\0'),
        _ => {
            if let Some(children) = node.children() {
                for child in children {
                    collect_inline(child, text);
                }
            } else {
                text.push('\0');
            }
        }
    }
}

fn target(source: &RepositoryName, raw: &str) -> Option<(RepositoryName, ItemNumber)> {
    let (repository, number) = if let Some(local) = raw.strip_prefix('#') {
        (source.clone(), local)
    } else if let Some((scheme, rest)) = raw.split_once("://") {
        if !scheme.eq_ignore_ascii_case("https") {
            return None;
        }
        let mut path = rest.trim_end_matches('/').split('/');
        let host = path.next()?;
        if !host.eq_ignore_ascii_case("github.com") {
            return None;
        }
        let repo = RepositoryName::new(path.next()?, path.next()?).ok()?;
        if path.next()? != "issues" {
            return None;
        }
        let number = path.next()?;
        if path.next().is_some() {
            return None;
        }
        (repo, number)
    } else {
        let (repo, number) = raw.split_once('#')?;
        let (owner, name) = repo.split_once('/')?;
        (RepositoryName::new(owner, name).ok()?, number)
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((
        repository,
        ItemNumber::try_from(number.parse::<u64>().ok()?).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_012_closing_markdown_normalizes_routes_and_skips_code_and_untrusted_destinations() {
        let source = RepositoryName::new("Example", "Main").unwrap();
        let body = "Resolves #41, FIXED: example/main#41. **Closes:** OTHER/Repo#27.\n\nClose https://GitHub.com/Example/Main/issues/9.\n\n\x60closes #7\x60\n\n\x60\x60\x60rust\nclose #8\n\x60\x60\x60\n\n    fixes #10\n\nFixes \x60code\x60 #18. 99 #1 close https://github.com.evil/a/b/issues/3 closed https://github.com/a/b/pull/4.\n\nResolves bad/repo#2?foo. fixed #0; unfixed #12. FIX: #5";
        let refs = closing_references(body, &source);
        let targets = refs
            .iter()
            .map(|r| (r.repository.owner(), r.repository.name(), r.number.get()))
            .collect::<Vec<_>>();
        assert_eq!(
            targets,
            [
                ("Example", "Main", 41),
                ("OTHER", "Repo", 27),
                ("Example", "Main", 9),
                ("Example", "Main", 5)
            ]
        );
        assert_eq!(refs[0].keyword, "Resolves");
        assert_eq!(refs[1].raw, "OTHER/Repo#27");
    }
}
