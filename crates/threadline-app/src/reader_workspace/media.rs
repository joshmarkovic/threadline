use super::*;
use crate::{palette::Palette, pane_content::markdown};
use gpui_kit::{
    AnyElement, Image, ImageFormat, ImageSource, IntoElement, ParentElement, Styled,
    component::Disableable, div,
};
use markdown::{ParseOptions, mdast::Node};
use std::collections::HashSet;
use threadline_storage::RepositoryId;

#[derive(Clone)]
struct Attachment {
    label: String,
    url: String,
    supported: bool,
}

pub(super) struct CachedMarkdown {
    text: Entity<TextViewState>,
    attachments: Vec<Attachment>,
}

pub(super) type ImageKey = (RepositoryId, String);

#[derive(Clone)]
pub(super) enum ImageState {
    Loading,
    Ready(Arc<Image>),
    Missing,
}

impl CachedMarkdown {
    pub(super) fn new(text: &str, cx: &mut Context<ReaderWorkspace>) -> Self {
        Self {
            text: cx.new(|cx| TextViewState::markdown(text, cx)),
            attachments: attachments(text),
        }
    }
}

impl ReaderWorkspace {
    fn request_image(
        &mut self,
        repository: RepositoryId,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        let key = (repository, url.clone());
        if matches!(
            self.images.get(&key),
            Some(ImageState::Loading | ImageState::Ready(_))
        ) {
            return;
        }
        self.images.insert(key.clone(), ImageState::Loading);
        let query = url.clone();
        self.launch(
            window,
            cx,
            move |store| store.cached_image(account, repository, &query),
            move |this, result, window, cx| {
                if let Ok(Some(cached)) = result
                    && let Some(image) = decoded_image(&cached.mime, cached.bytes)
                {
                    this.images.insert(key, ImageState::Ready(image));
                    return;
                }
                if !this.can_fetch() {
                    this.images.insert(key, ImageState::Missing);
                    return;
                }
                let Some(service) = this.service.clone() else {
                    return;
                };
                let epoch = this.generation;
                let cancel = this.network.clone();
                let store = Arc::clone(&this.store);
                let worker = this.runtime.spawn(async move {
                    let fetched = service
                        .fetch_image(account, repository, &url, &cancel)
                        .await
                        .ok()?;
                    let verified = fetched.mime;
                    let bytes = fetched.bytes;
                    let image = decoded_image(verified, bytes.clone())?;
                    tokio::task::spawn_blocking(move || {
                        store.cache_image(account, repository, &url, verified, &bytes, now_ms())
                    })
                    .await
                    .ok()?
                    .ok()?;
                    Some(image)
                });
                cx.spawn_in(window, async move |view, cx| {
                    let result = worker.await.ok().flatten();
                    let _ = view.update_in(cx, move |this, _, cx| {
                        if this.generation != epoch {
                            return;
                        }
                        if this.images.len() >= 32 {
                            this.images
                                .retain(|_, state| matches!(state, ImageState::Loading));
                        }
                        this.images
                            .insert(key, result.map_or(ImageState::Missing, ImageState::Ready));
                        cx.notify();
                    });
                })
                .detach();
            },
        );
        cx.notify();
    }

    pub(super) fn markdown_element(
        &self,
        document: &CachedMarkdown,
        repository: Option<RepositoryId>,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let within = document
            .attachments
            .iter()
            .filter_map(|attachment| {
                let key = (repository?, attachment.url.clone());
                let ImageState::Ready(image) = self.images.get(&key)? else {
                    return None;
                };
                Some((attachment.url.clone(), image.clone()))
            })
            .collect::<HashMap<_, _>>();
        let weak = cx.weak_entity();
        let text = markdown(&document.text, palette)
            .image_source(move |uri| {
                within.get(uri.as_ref()).cloned().map_or_else(
                    || ImageSource::Custom(Arc::new(|_, _| None)),
                    ImageSource::Image,
                )
            })
            .on_link_click(move |url, _, window, cx| {
                if super::view::parse_item_url(url).is_some() {
                    let _ = weak.update(cx, |this, cx| this.navigate_url(url, window, cx));
                } else if ordinary_github_link(url) {
                    cx.open_url(url);
                }
            });
        let controls = document
            .attachments
            .iter()
            .enumerate()
            .map(|(index, attachment)| {
                let key = repository.map(|repo| (repo, attachment.url.clone()));
                let state = key.as_ref().and_then(|key| self.images.get(key));
                let label = match state {
                    Some(ImageState::Ready(_)) => "Image downloaded",
                    Some(ImageState::Loading) => "Loading image…",
                    Some(ImageState::Missing) => "Image unavailable. Retry",
                    None if self.offline || self.access == AccessState::SignedOut => {
                        "Open cached image"
                    }
                    None => "Load image",
                };
                let caption = if attachment.label.trim().is_empty() {
                    "Image attachment"
                } else {
                    attachment.label.as_str()
                };
                div()
                    .flex_wrap()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(caption.to_owned()),
                    )
                    .child(match key.filter(|_| attachment.supported) {
                        Some((repo, url)) => {
                            super::view::control(format!("reader-attachment-{index}"), label)
                                .disabled(matches!(
                                    state,
                                    Some(ImageState::Loading | ImageState::Ready(_))
                                ))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.request_image(repo, url.clone(), window, cx)
                                }))
                                .into_any_element()
                        }
                        None => div()
                            .text_xs()
                            .text_color(palette.muted)
                            .child("Unsupported attachment")
                            .into_any_element(),
                    })
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(text)
            .children(controls)
            .into_any_element()
    }
}

fn ordinary_github_link(url: &str) -> bool {
    url.starts_with("https://github.com/")
        && !url.contains(['\\', '\r', '\n', '\t'])
        && !supported_image_url(url)
}

fn supported_image_url(url: &str) -> bool {
    !url.contains(['?', '#', '\\', '\r', '\n', '\t'])
        && [
            "https://github.com/user-attachments/assets/",
            "https://user-images.githubusercontent.com/",
            "https://private-user-images.githubusercontent.com/",
            "https://github.com/assets/",
        ]
        .iter()
        .any(|prefix| {
            url.strip_prefix(prefix)
                .is_some_and(|path| !path.is_empty())
        })
}

fn attachments(text: &str) -> Vec<Attachment> {
    let end = text.floor_char_boundary(text.len().min(128 * 1024));
    let Ok(root) = markdown::to_mdast(&text[..end], &ParseOptions::gfm()) else {
        return vec![];
    };
    let mut nodes = vec![&root];
    let mut definitions = HashMap::new();
    let mut entries = Vec::new();
    while let Some(node) = nodes.pop() {
        match node {
            Node::Definition(definition) => {
                definitions
                    .entry(&definition.identifier)
                    .or_insert(&definition.url);
            }
            Node::Image(image) => entries.push((image.alt.clone(), Some(image.url.as_str()), None)),
            Node::ImageReference(reference) => {
                entries.push((reference.alt.clone(), None, Some(&reference.identifier)))
            }
            Node::Link(link) if link.url.contains("/user-attachments/") => {
                entries.push((String::from("File attachment"), Some(&link.url), None));
            }
            _ => {}
        }
        if let Some(children) = node.children() {
            nodes.extend(children.iter().rev());
        }
    }
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter_map(|(label, url, reference)| {
            let url = url.or_else(|| {
                reference.and_then(|name| definitions.get(name).map(|url| url.as_str()))
            })?;
            seen.insert(url).then(|| Attachment {
                label,
                url: url.to_owned(),
                supported: supported_image_url(url),
            })
        })
        .take(12)
        .collect()
}

fn decoded_image(mime: &str, bytes: Vec<u8>) -> Option<Arc<Image>> {
    let format = match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        _ => return None,
    };
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_controls_identify_references_and_keep_missing_or_unsupported_media_explicit() {
        let sample = concat!(
            "![missing local attachment](../missing.jpg)\n",
            "![network tracking image](https://example.net/track.png)\n",
            "![user granted load][img]\n",
            "![signed](https://user-images.githubusercontent.com/image.png?signature=synthetic)\n\n",
            "[img]: https://github.com/user-attachments/assets/synthetic\n"
        );
        let entries = attachments(sample);
        assert_eq!(entries.len(), 4);
        assert!(!entries[0].supported);
        assert!(!entries[1].supported);
        assert!(entries[2].supported);
        assert!(!entries[3].supported);
        assert!(!ordinary_github_link(
            "https://github.com/user-attachments/assets/synthetic"
        ));
        assert!(!ordinary_github_link("javascript:alert(1)"));
    }
}
