//! Cover art URLs. The API hands out `avatars.yandex.net/get-music-content/…/%%` templates.

/// Sizes the avatar CDN serves for music content.
pub const SIZES: [u32; 9] = [50, 100, 200, 300, 400, 460, 600, 800, 1000];

/// The smallest CDN size that is at least `px` (clamped to the largest).
pub fn snap_size(px: u32) -> u32 {
    SIZES.iter().copied().find(|s| *s >= px).unwrap_or(1000)
}

/// Turns a `…/%%` template into a fetchable https URL of `size`×`size`.
pub fn url(template: &str, size: u32) -> String {
    let size = snap_size(size);
    let t = template.replace("%%", &format!("{size}x{size}"));
    if t.starts_with("http://") || t.starts_with("https://") {
        t
    } else if let Some(rest) = t.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        format!("https://{t}")
    }
}

/// The hosts we are willing to fetch artwork, lyrics and audio from. Anything else is ignored,
/// so a hostile response cannot make the client request arbitrary URLs.
pub fn is_trusted_host(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or("");
    host == "yandex.net" || host.ends_with(".yandex.net") || host.ends_with(".yandex.ru") || host.ends_with(".yandex.com") || host.ends_with(".yastatic.net")
}

/// Hosts allowed for album/track artwork: Yandex CDNs plus FkCensorData cover mirrors.
pub fn is_trusted_image_host(url: &str) -> bool {
    is_trusted_host(url) || url.starts_with("https://raw.githubusercontent.com/fcensor-data/") || url.starts_with("https://t2.genius.com/unsafe/")
}
