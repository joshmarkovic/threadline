use super::digest;
use crate::report::sha256_reader;
use object::{Architecture, Object, read::pe::PeFile64};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Configuration {
    pub(super) dependency_tree: PathBuf,
    dependency_sha256: String,
    app_source_revision: String,
}

const BROWSER_PREFIXES: &[&str] = &[
    "awesomium",
    "cef-",
    "cef_",
    "chromedriver",
    "chromium",
    "chrome.exe",
    "chrome.dll",
    "chrome_elf",
    "edgehtml",
    "electron",
    "geckoview",
    "ieframe",
    "libcef",
    "libxul",
    "msedge",
    "mshtml",
    "microsoft.web.webview",
    "qt5web",
    "qt6web",
    "qtweb",
    "servo-webview",
    "tauri-runtime-wry",
    "webengine",
    "webkit",
    "webview",
    "wry-",
    "xul.dll",
    "xpcom.dll",
];

fn browser(name: &str) -> bool {
    matches!(name, "cef" | "wry" | "tauri" | "servo" | "gecko")
        || BROWSER_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

fn normal_dependencies(raw: &str) -> Result<BTreeSet<String>, &'static str> {
    let mut names = BTreeSet::new();
    for line in raw.lines() {
        let mut fields = line.split_whitespace();
        let name = fields.next().ok_or("Empty dependency inventory row.")?;
        if !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || !fields
                .next()
                .is_some_and(|version| version.starts_with('v') && version.len() > 1)
        {
            return Err(
                "Use the exact Windows cargo tree with --prefix none and normal dependencies.",
            );
        }
        names.insert(name.to_ascii_lowercase());
    }
    if !["threadline-app", "gpui-kit", "markdown"]
        .iter()
        .all(|name| names.contains(*name))
    {
        return Err(
            "The supplied dependency graph lacks the expected native app and Markdown packages.",
        );
    }
    Ok(names)
}

pub(super) fn inventory(
    app: &Path,
    graph: &Path,
    config: &Configuration,
    app_hash: &str,
    dir: &Path,
) -> Result<bool, &'static str> {
    if !digest(&config.dependency_sha256)
        || config.app_source_revision.len() != 40
        || !config
            .app_source_revision
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(
            "Supply the exact build source commit and independent Windows dependency digest.",
        );
    }
    let raw =
        fs::read(graph).map_err(|_| "The selected package dependency inventory is unavailable.")?;
    let actual =
        sha256_reader(raw.as_slice()).map_err(|_| "Cannot hash the dependency inventory.")?;
    if !actual.eq_ignore_ascii_case(&config.dependency_sha256) {
        return Err("The package dependency inventory differs from its recorded digest.");
    }
    let dependencies = normal_dependencies(
        std::str::from_utf8(&raw).map_err(|_| "The dependency inventory is not UTF-8.")?,
    )?;
    let bytes = fs::read(app).map_err(|_| "The installed executable could not be read.")?;
    if !sha256_reader(bytes.as_slice())
        .map_err(|_| "Cannot hash the installed executable.")?
        .eq_ignore_ascii_case(app_hash)
    {
        return Err("The installed bytes changed before static import inspection.");
    }
    let pe = PeFile64::parse(bytes.as_slice())
        .map_err(|_| "The installed executable is not a readable 64-bit PE.")?;
    if pe.architecture() != Architecture::X86_64 {
        return Err("The installed executable is not native Windows x64.");
    }
    let mut imported = BTreeSet::new();
    for import in pe
        .imports()
        .map_err(|_| "Windows PE import table inspection failed.")?
    {
        let import = import.map_err(|_| "A Windows PE import entry was invalid.")?;
        let name =
            std::str::from_utf8(import.library()).map_err(|_| "Invalid Windows import name.")?;
        imported.insert(name.to_ascii_lowercase());
    }
    if imported.is_empty() {
        return Err("No Windows PE import library could be corroborated.");
    }
    let matches: BTreeSet<_> = imported
        .iter()
        .chain(&dependencies)
        .filter(|s| browser(s))
        .collect();
    let report = json!({
        "app_sha256":app_hash,"source_commit":config.app_source_revision,"dependency_tree_sha256":actual,
        "normal_dependency_names":dependencies.len(),"native_ui":"gpui-kit","markdown_dependency":"markdown",
        "imported_libraries":imported,"browser_matches":matches,"browser_prefixes":BROWSER_PREFIXES,
    });
    fs::write(
        dir.join("runtime-build.json"),
        serde_json::to_vec_pretty(&report).expect("JSON inventory values are strings and numbers"),
    )
    .map_err(|_| "Could not save the packaged build inventory.")?;
    Ok(matches.is_empty())
}

pub(super) fn corroborates(receipt: &Value) -> bool {
    let samples = receipt["observations"].as_array();
    receipt["case"] == "E2E-020"
        && receipt["status"] == "Pass"
        && receipt["synthetic_profile_archived"] == true
        && receipt["markdown_initial"] == "Open cached image"
        && receipt["markdown_after"] == "Image unavailable. Retry"
        && receipt["selected_file"] == "second.txt"
        && receipt["diff_mode"] == "Split"
        && receipt["offline"] == true
        && receipt["orderly_quit"] == true
        && samples.is_some_and(|rows| {
            rows.len() >= 3
                && rows.iter().all(|row| {
                    row["accessible_root_pid"] == receipt["process_id"]
                        && row["module_count"].as_u64().is_some_and(|count| count > 0)
                        && row["browser_matches"].as_array().is_some_and(Vec::is_empty)
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_runtime_inventory_rejects_incomplete_input_and_identifies_browser_components() {
        assert!(normal_dependencies("gpui-kit v1.0").is_err());
        let names = normal_dependencies(
            "threadline-app v0.1 (C:/build)\ngpui-kit v0.10\nmarkdown v1.0\nwry v0.1",
        )
        .unwrap();
        assert!(names.iter().any(|s| browser(s)));
        assert!(browser("msedgewebview2.exe"));
        assert!(browser("libcef.dll"));
        assert!(!browser("d3d11.dll"));
    }
}
