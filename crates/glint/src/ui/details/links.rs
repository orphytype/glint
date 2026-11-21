//! Artist external link recognition, platform table, and pill widget.

use rune::{Cursor, El, Icon, Tx, row};

use super::icons;
use super::kit::{Hover as _, label};
use crate::app::State;
use crate::i18n::Lang;
use crate::theme as th;

struct Platform {
    icon: &'static Icon,
    en: &'static str,
    ru: &'static str,
    domains: &'static [&'static str],
    networks: &'static [&'static str],
}

impl Platform {
    fn label(&self, lang: Lang) -> &'static str {
        if matches!(lang, Lang::Ru) { self.ru } else { self.en }
    }
}

/// Ordered most-specific first (`music.youtube.com` before `youtube.com`, `music.vk.*` before `vk.*`).
const PLATFORMS: &[Platform] = &[
    Platform { icon: &icons::SOCIAL_YOUTUBE, en: "YouTube Music", ru: "YouTube Music", domains: &["music.youtube.com"], networks: &["youtube_music", "youtubemusic"] },
    Platform { icon: &icons::SOCIAL_YOUTUBE, en: "YouTube", ru: "YouTube", domains: &["youtube.com", "youtu.be", "youtube-nocookie.com"], networks: &["youtube", "yt"] },
    Platform { icon: &icons::SOCIAL_VK, en: "VK Music", ru: "VK Музыка", domains: &["music.vk.com", "music.vk.ru"], networks: &[] },
    Platform { icon: &icons::SOCIAL_VK, en: "VK Video", ru: "VK Видео", domains: &["vkvideo.ru"], networks: &[] },
    Platform { icon: &icons::SOCIAL_VK, en: "VK", ru: "VK", domains: &["vk.com", "vk.ru", "vk.me", "vk.cc"], networks: &["vk", "vkontakte"] },
    Platform {
        icon: &icons::SOCIAL_TELEGRAM,
        en: "Telegram",
        ru: "Telegram",
        domains: &["t.me", "telegram.me", "telegram.org", "telegram.dog", "tg.me", "telesco.pe"],
        networks: &["telegram", "tg"],
    },
    Platform { icon: &icons::SOCIAL_TIKTOK, en: "TikTok", ru: "TikTok", domains: &["tiktok.com", "tiktokv.com"], networks: &["tiktok", "tik_tok", "tik-tok"] },
    Platform { icon: &icons::SOCIAL_INSTAGRAM, en: "Instagram", ru: "Instagram", domains: &["instagram.com", "instagr.am"], networks: &["instagram", "insta", "ig"] },
    Platform { icon: &icons::SOCIAL_X, en: "X", ru: "X", domains: &["x.com", "twitter.com", "t.co"], networks: &["twitter", "x"] },
    Platform { icon: &icons::SOCIAL_TWITCH, en: "Twitch", ru: "Twitch", domains: &["twitch.tv"], networks: &["twitch"] },
    Platform { icon: &icons::SOCIAL_SPOTIFY, en: "Spotify", ru: "Spotify", domains: &["spotify.com", "spoti.fi"], networks: &["spotify"] },
    Platform { icon: &icons::SOCIAL_SOUNDCLOUD, en: "SoundCloud", ru: "SoundCloud", domains: &["soundcloud.com", "snd.sc"], networks: &["soundcloud"] },
    Platform { icon: &icons::SOCIAL_BANDCAMP, en: "Bandcamp", ru: "Bandcamp", domains: &["bandcamp.com"], networks: &["bandcamp"] },
    Platform { icon: &icons::SOCIAL_DISCORD, en: "Discord", ru: "Discord", domains: &["discord.gg", "discord.com", "discordapp.com"], networks: &["discord"] },
    Platform { icon: &icons::SOCIAL_FACEBOOK, en: "Facebook", ru: "Facebook", domains: &["facebook.com", "fb.com", "fb.me"], networks: &["facebook", "fb"] },
    Platform { icon: &icons::SOCIAL_OK, en: "OK", ru: "Одноклассники", domains: &["ok.ru", "odnoklassniki.ru"], networks: &["ok", "odnoklassniki"] },
    Platform { icon: &icons::SOCIAL_DZEN, en: "Dzen", ru: "Дзен", domains: &["dzen.ru", "zen.yandex.ru", "zen.yandex.com"], networks: &["dzen", "zen", "yandex_zen"] },
    Platform { icon: &icons::NOTE, en: "Apple Music", ru: "Apple Music", domains: &["music.apple.com", "itunes.apple.com"], networks: &["apple_music", "applemusic", "itunes"] },
    Platform {
        icon: &icons::NOTE,
        en: "Yandex Music",
        ru: "Яндекс Музыка",
        domains: &["music.yandex.ru", "music.yandex.com", "music.yandex.by", "music.yandex.kz"],
        networks: &["yandex_music"],
    },
    Platform { icon: &icons::NOTE, en: "Zvuk", ru: "Звук", domains: &["zvuk.com", "sber-zvuk.com"], networks: &["zvuk"] },
    Platform { icon: &icons::NOTE, en: "MTS Music", ru: "МТС Музыка", domains: &["music.mts.ru"], networks: &["mts_music"] },
    Platform { icon: &icons::NOTE, en: "Deezer", ru: "Deezer", domains: &["deezer.com", "deezer.page.link"], networks: &["deezer"] },
    Platform { icon: &icons::NOTE, en: "Tidal", ru: "Tidal", domains: &["tidal.com"], networks: &["tidal"] },
    Platform { icon: &icons::NOTE, en: "Shazam", ru: "Shazam", domains: &["shazam.com"], networks: &["shazam"] },
    Platform { icon: &icons::NOTE, en: "Genius", ru: "Genius", domains: &["genius.com"], networks: &["genius"] },
    Platform { icon: &icons::NOTE, en: "Last.fm", ru: "Last.fm", domains: &["last.fm", "lastfm.ru", "lastfm.com"], networks: &["lastfm", "last_fm", "last.fm"] },
    Platform { icon: &icons::NOTE, en: "Discogs", ru: "Discogs", domains: &["discogs.com"], networks: &["discogs"] },
    Platform { icon: &icons::NOTE, en: "Rate Your Music", ru: "Rate Your Music", domains: &["rateyourmusic.com"], networks: &[] },
    Platform { icon: &icons::NOTE, en: "MusicBrainz", ru: "MusicBrainz", domains: &["musicbrainz.org"], networks: &[] },
    Platform { icon: &icons::NOTE, en: "AllMusic", ru: "AllMusic", domains: &["allmusic.com"], networks: &[] },
    Platform { icon: &icons::NOTE, en: "PromoDJ", ru: "PromoDJ", domains: &["promodj.com"], networks: &["promodj"] },
    Platform { icon: &icons::NOTE, en: "Mixcloud", ru: "Mixcloud", domains: &["mixcloud.com"], networks: &[] },
    Platform { icon: &icons::NOTE, en: "Audiomack", ru: "Audiomack", domains: &["audiomack.com"], networks: &[] },
    Platform { icon: &icons::NOTE, en: "Beatport", ru: "Beatport", domains: &["beatport.com"], networks: &[] },
    Platform { icon: &icons::NOTE, en: "Resident Advisor", ru: "Resident Advisor", domains: &["ra.co", "residentadvisor.net"], networks: &[] },
    Platform { icon: &icons::EXTERNAL, en: "Rutube", ru: "Rutube", domains: &["rutube.ru"], networks: &["rutube"] },
    Platform { icon: &icons::EXTERNAL, en: "Threads", ru: "Threads", domains: &["threads.net", "threads.com"], networks: &["threads"] },
    Platform { icon: &icons::EXTERNAL, en: "BandLink", ru: "BandLink", domains: &["band.link", "bandlink.ru"], networks: &["bandlink", "band_link"] },
    Platform { icon: &icons::EXTERNAL, en: "Linktree", ru: "Linktree", domains: &["linktr.ee", "linktree.com"], networks: &["linktree"] },
    Platform { icon: &icons::EXTERNAL, en: "Taplink", ru: "Taplink", domains: &["taplink.cc", "taplink.ru", "taplink.at"], networks: &["taplink"] },
    Platform { icon: &icons::EXTERNAL, en: "Reddit", ru: "Reddit", domains: &["reddit.com", "redd.it"], networks: &["reddit"] },
    Platform { icon: &icons::EXTERNAL, en: "Pinterest", ru: "Pinterest", domains: &["pinterest.com", "pinterest.ru", "pin.it"], networks: &["pinterest"] },
    Platform { icon: &icons::EXTERNAL, en: "Likee", ru: "Likee", domains: &["likee.video", "likee.com"], networks: &["likee"] },
    Platform { icon: &icons::EXTERNAL, en: "Snapchat", ru: "Snapchat", domains: &["snapchat.com"], networks: &["snapchat"] },
    Platform { icon: &icons::EXTERNAL, en: "Kick", ru: "Kick", domains: &["kick.com"], networks: &["kick"] },
    Platform { icon: &icons::EXTERNAL, en: "Tumblr", ru: "Tumblr", domains: &["tumblr.com"], networks: &["tumblr"] },
    Platform { icon: &icons::EXTERNAL, en: "Vimeo", ru: "Vimeo", domains: &["vimeo.com"], networks: &["vimeo"] },
    Platform { icon: &icons::EXTERNAL, en: "Bluesky", ru: "Bluesky", domains: &["bsky.app", "bsky.social"], networks: &["bluesky", "bsky"] },
    Platform { icon: &icons::EXTERNAL, en: "Mastodon", ru: "Mastodon", domains: &["mastodon.social", "mastodon.online"], networks: &["mastodon"] },
    Platform { icon: &icons::EXTERNAL, en: "WhatsApp", ru: "WhatsApp", domains: &["whatsapp.com", "wa.me"], networks: &["whatsapp"] },
    Platform { icon: &icons::EXTERNAL, en: "Yappy", ru: "Yappy", domains: &["yappy.media"], networks: &["yappy"] },
    Platform { icon: &icons::EXTERNAL, en: "Coub", ru: "Coub", domains: &["coub.com"], networks: &["coub"] },
    Platform { icon: &icons::EXTERNAL, en: "Wikipedia", ru: "Википедия", domains: &["wikipedia.org"], networks: &["wikipedia", "wiki"] },
    Platform { icon: &icons::EXTERNAL, en: "Songkick", ru: "Songkick", domains: &["songkick.com"], networks: &[] },
    Platform { icon: &icons::EXTERNAL, en: "Bandsintown", ru: "Bandsintown", domains: &["bandsintown.com"], networks: &[] },
    Platform { icon: &icons::EXTERNAL, en: "Bilibili", ru: "Bilibili", domains: &["bilibili.com"], networks: &[] },
    Platform { icon: &icons::EXTERNAL, en: "Weibo", ru: "Weibo", domains: &["weibo.com", "weibo.cn"], networks: &[] },
    Platform { icon: &icons::EXTERNAL, en: "GitHub", ru: "GitHub", domains: &["github.com"], networks: &["github"] },
    Platform { icon: &icons::EXTERNAL, en: "Steam", ru: "Steam", domains: &["steampowered.com", "steamcommunity.com"], networks: &["steam"] },
    Platform { icon: &icons::EXTERNAL, en: "Myspace", ru: "Myspace", domains: &["myspace.com"], networks: &["myspace"] },
    Platform { icon: &icons::EXTERNAL, en: "LiveJournal", ru: "LiveJournal", domains: &["livejournal.com"], networks: &["livejournal"] },
];

/// Extracts the lowercased host and raw path from a link URL (handles missing schemes and `tg://`).
fn url_host_and_path(href: &str) -> (Option<String>, Option<&str>) {
    let s = href.trim();
    if s.is_empty() {
        return (None, None);
    }
    if s.len() >= 5 && s[..5].eq_ignore_ascii_case("tg://") {
        return (Some("t.me".into()), None);
    }
    if s.len() >= 7 && s[..7].eq_ignore_ascii_case("mailto:") {
        return (None, None);
    }
    let rest = if let Some((_, after)) = s.split_once("://") {
        after
    } else if let Some(after) = s.strip_prefix("//") {
        after
    } else {
        s
    };
    let (authority, path_part) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], Some(&rest[i..])),
        None => (rest, None),
    };
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = host_port.split(':').next().unwrap_or("").trim_end_matches('.').to_ascii_lowercase();
    let host = (!host.is_empty()).then_some(host);
    let path = path_part.map(|p| p.split(['?', '#']).next().unwrap_or(""));
    (host, path)
}

fn host_matches(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|p| p.ends_with('.'))
}

fn recognize_host(host: &str, lang: Lang) -> Option<(&'static Icon, &'static str)> {
    PLATFORMS.iter().find(|p| p.domains.iter().any(|d| host_matches(host, d))).map(|p| (p.icon, p.label(lang)))
}

fn recognize_network(net: &str, lang: Lang) -> Option<(&'static Icon, &'static str)> {
    let key = net.trim().to_ascii_lowercase();
    PLATFORMS.iter().find(|p| p.networks.iter().any(|&n| n == key)).map(|p| (p.icon, p.label(lang)))
}

/// True when the API's link title is a raw URL, handle, or URL path rather than a display name.
fn is_raw_title(title: &str, path: Option<&str>) -> bool {
    let lower = title.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") || title.starts_with('@') || title.starts_with('/') {
        return true;
    }
    if !title.contains(' ') && title.contains('/') {
        return true;
    }
    path.map(|p| p.trim_matches('/')).filter(|p| !p.is_empty()).is_some_and(|p| title.trim_matches('/') == p)
}

/// Icon and label for an artist link pill: recognized platform first, then human title or host.
pub(super) fn link_badge(l: &ym_api::ArtistLink, lang: Lang) -> (&'static Icon, String) {
    let (host, path) = url_host_and_path(&l.href);
    if let Some((ic, name)) = host.as_deref().and_then(|h| recognize_host(h, lang)).or_else(|| l.social_network.as_deref().and_then(|n| recognize_network(n, lang))) {
        return (ic, name.to_owned());
    }
    let title = l.title.trim();
    if !title.is_empty() && !is_raw_title(title, path) {
        return (&icons::EXTERNAL, title.to_owned());
    }
    if let Some(h) = host {
        return (&icons::EXTERNAL, h.strip_prefix("www.").unwrap_or(&h).to_owned());
    }
    (&icons::EXTERNAL, if !title.is_empty() { title.to_owned() } else { l.href.clone() })
}

/// Renders an artist link or donation button as a pill.
pub(super) fn link_pill(key: &str, ic: &'static Icon, title: String, url: &str, accent: bool) -> El {
    let url = url.to_owned();
    let h = 34.0;
    row()
        .key(key.to_owned())
        .role("link")
        .label(title.as_str())
        .help(url.as_str())
        .h(h)
        .pl(11.0)
        .pr(13.0)
        .gap(7.0)
        .radius(h * 0.5)
        .items_center()
        .cursor(Cursor::Pointer)
        .fill(if accent { th::rgba(0xe8394a, 0.22) } else { th::veil(0.06) })
        .hov(if accent { th::rgba(0xe8394a, 0.32) } else { th::veil(0.1) })
        .press_scale(th::PRESS)
        .on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let _ = tx.open_url(&url);
        })
        .child(rune::icon(ic, 15.0).color(if accent { th::rgb(0xff8a95) } else { th::text2() }))
        .child(label(title, th::BODY_M, th::text()))
}
