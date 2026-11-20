use std::sync::{Arc, LazyLock};

use gpui_kit::{Image, ImageFormat, Img, Pixels, Styled, img};

static MARK: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../assets/threadline-mark.png").to_vec(),
    ))
});

/// The application icon, drawn from the same artwork as the Windows icon.
pub(crate) fn mark(size: Pixels) -> Img {
    img(MARK.clone()).size(size).flex_shrink_0()
}
