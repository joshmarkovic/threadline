//! Explicit image requests use a separate HTTPS client without GitHub API credentials.

use std::{fmt, time::Duration};

use reqwest::{Client, Url, header, redirect};

use super::CancellationToken;

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageError {
    UnsupportedAddress,
    UnsupportedType,
    Unavailable,
    TooLarge,
    Cancelled,
}

impl fmt::Display for ImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedAddress => "This attachment cannot be loaded inside Threadline.",
            Self::UnsupportedType => "This image format has no preview.",
            Self::Unavailable => "The attachment is unavailable. A private attachment may require GitHub in your browser.",
            Self::TooLarge => "The image is too large to preview.",
            Self::Cancelled => "The attachment request was cancelled.",
        })
    }
}

impl std::error::Error for ImageError {}

pub async fn fetch_image(raw: &str, cancel: &CancellationToken) -> Result<Vec<u8>, ImageError> {
    if raw.len() > 8192 {
        return Err(ImageError::UnsupportedAddress);
    }
    let mut url = Url::parse(raw).map_err(|_| ImageError::UnsupportedAddress)?;
    check_attachment_address(&url)?;
    let client = Client::builder()
        .user_agent(concat!("Threadline/", env!("CARGO_PKG_VERSION")))
        .redirect(redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .https_only(true)
        .build()
        .map_err(|_| ImageError::Unavailable)?;
    for redirects in 0..=MAX_REDIRECTS {
        let request = client
            .get(url.clone())
            .header(header::ACCEPT, "image/png,image/jpeg");
        let mut response = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(ImageError::Cancelled),
            response = request.send() => response.map_err(|_| ImageError::Unavailable)?,
        };
        if response.status().is_redirection() {
            if redirects == MAX_REDIRECTS {
                return Err(ImageError::Unavailable);
            }
            let target = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(ImageError::Unavailable)?;
            let next = url
                .join(target)
                .map_err(|_| ImageError::UnsupportedAddress)?;
            check_attachment_address(&next)?;
            url = next;
            continue;
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err(ImageError::Unavailable);
        }
        let media = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or("");
        if !matches!(
            media,
            "image/jpeg" | "image/png" | "application/octet-stream" | ""
        ) {
            return Err(ImageError::UnsupportedType);
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_IMAGE_BYTES as u64)
        {
            return Err(ImageError::TooLarge);
        }
        let mut bytes = Vec::new();
        loop {
            let chunk = tokio::select! { biased;
                _ = cancel.cancelled() => return Err(ImageError::Cancelled),
                chunk = response.chunk() => chunk.map_err(|_| ImageError::Unavailable)?,
            };
            let Some(chunk) = chunk else {
                break;
            };
            if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
                return Err(ImageError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(bytes);
    }
    Err(ImageError::Unavailable)
}

fn check_attachment_address(url: &Url) -> Result<(), ImageError> {
    if url.scheme() != "https"
        || url.port().is_some_and(|port| port != 443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ImageError::UnsupportedAddress);
    }
    let supported = match url.host_str() {
        Some("github.com") => url
            .path()
            .strip_prefix("/user-attachments/assets/")
            .is_some_and(|id| {
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }),
        Some(
            "user-images.githubusercontent.com"
            | "private-user-images.githubusercontent.com"
            | "camo.githubusercontent.com",
        ) => url.path() != "/",
        _ => false,
    };
    if !supported {
        return Err(ImageError::UnsupportedAddress);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachments_use_only_expected_https_locations() {
        for url in [
            "https://github.com/user-attachments/assets/111",
            "https://user-images.githubusercontent.com/111/example.png",
            "https://private-user-images.githubusercontent.com/111/example.png?synthetic=signed",
        ] {
            assert!(check_attachment_address(&Url::parse(url).unwrap()).is_ok());
        }
        for url in [
            "http://github.com/user-attachments/assets/111",
            "https://github.com/settings/tokens",
            "https://github.com/user-attachments/assets/../private",
            "https://github.com/user-attachments/assets/%2f..%2fprivate",
            "https://github.com@elsewhere.test/user-attachments/assets/111",
            "https://github.com:444/user-attachments/assets/111",
            "https://local@user-images.githubusercontent.com/111/example.png",
            "https://usercontent.example/111/example.png",
            "file:///private/profile.sqlite3",
        ] {
            assert_eq!(
                check_attachment_address(&Url::parse(url).unwrap()),
                Err(ImageError::UnsupportedAddress)
            );
        }
    }
}
