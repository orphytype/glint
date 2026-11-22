//! Right-column content sections for the details modal (credits, release, listeners, links, photos, facts).

use rune::{Align, Color, Cursor, El, Tx, col, el, row};
use ym_api::Track;

use super::icons;
use super::kit::{self, Hover as _, label};
use super::links::{link_badge, link_pill};
use crate::app::{Load, Page, Sheet, State};
use crate::i18n::S;
use crate::theme as th;

const SIM_MIN_W: f32 = 100.0;
const SIM_PIC: f32 = 68.0;
const SIM_H: f32 = SIM_PIC + 10.0 + 36.0;
const SIM_GAP: f32 = 10.0;
const PHOTO_COLS: usize = 3;
const PHOTO_MAX: usize = 6;
const UP: u32 = 0x4fd18b;
const DOWN: u32 = 0xf07c7c;

/// Closes the details modal and navigates to `page`.
pub(super) fn go_to(s: &mut State, page: Page) {
    s.sheet_open = false;
    s.go(page);
}

/// A labeled section in the modal's right column.
fn section(key: &str, name: &str, body: El) -> El {
    col().key(key.to_owned()).w_full().gap(12.0).child(kit::caps(name, th::text3())).child(body)
}

/// One key/value fact row.
fn fact_line(key: String, name: impl Into<String>, value: impl Into<String>, width: f32) -> El {
    col().key(key).w(width).gap(3.0).child(label(name.into(), th::BODY_M, th::text()).wrap().w(width)).child(label(value.into(), th::BODY, th::text2()).wrap().w(width))
}

/// Shimmering placeholder lines while track credits load.
fn bones(key: &str, n: usize, width: f32) -> El {
    let widths = [0.42, 0.78, 0.5, 0.66, 0.36];
    col().key(key.to_owned()).w(width).gap(10.0).children((0..n).map(|i| {
        el().w(width * widths[i % widths.len()]).h(if i % 2 == 0 { 14.0 } else { 12.0 }).radius(6.0).fill(th::veil(0.08)).looping(rune::LoopSpec::opacity(
            1.4,
            0.08 * i as f64,
            0.45,
            1.0,
        ))
    }))
}

/// Clickable row leading to a release page or external link.
fn lead_row(key: &str, pic: El, title: String, sub: Option<String>, out: bool, width: f32) -> El {
    let text_w = width - 20.0 - 52.0 - 24.0 - 20.0;
    let mut words = col().grow(1.0).min_w(0.0).gap(2.0).child(label(title, th::BODY_M, th::text()).truncate().w(text_w));
    if let Some(sub) = sub.filter(|x| !x.is_empty()) {
        words = words.child(label(sub, th::SMALL, th::text3()).truncate().w(text_w));
    }
    row()
        .key(key.to_owned())
        .role("link")
        .w(width)
        .px(10.0)
        .py(10.0)
        .gap(12.0)
        .items_center()
        .radius(16.0)
        .cursor(Cursor::Pointer)
        .fill(th::veil(0.045))
        .hov(th::veil(0.08))
        .press(th::veil(0.11))
        .press_scale(th::PRESS)
        .hov_group()
        .child(pic)
        .child(words)
        .child(row().w(20.0).h(20.0).center().child(rune::icon(if out { &icons::EXTERNAL } else { &icons::CHEVRON_RIGHT }, 16.0).color(th::text3()).gcol(th::text())))
}

fn glyph_disc(ic: &'static rune::Icon, fill: Color) -> El {
    row().w(52.0).h(52.0).radius(26.0).center().fill(fill).child(rune::icon(ic, 20.0).color(th::rgb(0xffffff)))
}

fn credits(s: &State, t: &Track, width: f32) -> El {
    let lang = s.lang;
    let mine = s.sheet_credits.0.as_deref() == Some(t.id.as_str());
    let list = match &s.sheet_credits.1 {
        Load::Ready(c) if mine && !c.is_empty() => {
            col().w(width).gap(14.0).children(c.iter().enumerate().map(|(i, c)| fact_line(format!("sheet-credit-{i}"), c.title.as_str(), c.value.as_str(), width)))
        }
        Load::Loading | Load::Idle if mine || s.sheet_credits.0.is_none() => return section("sheet-credits", S::SheetCredits.get(lang), bones("sheet-credits-bones", 6, width)),
        _ => col().w(width).gap(14.0).child(fact_line("sheet-credit-0".into(), S::Artist.get(lang), t.artists_line(), width)),
    };
    section("sheet-credits", S::SheetCredits.get(lang), list)
}

fn release(s: &State, t: &Track, width: f32) -> Option<El> {
    let al = t.albums.first()?;
    let lang = s.lang;
    let mut sub = vec![S::Album.get(lang).to_owned()];
    if let Some(y) = al.year {
        sub.push(y.to_string());
    }
    if let Some(n) = al.track_count.filter(|n| *n > 1) {
        sub.push(format!("{n} {}", S::Tracks.get(lang).to_lowercase()));
    }
    let page = Page::Album(al.id.clone());
    let pic = el().w(52.0).h(52.0).radius(10.0).child(kit::cover_sm(s, al.cover_template().or(t.cover_template()), 52.0, 10.0));
    let r = lead_row("sheet-release", pic, al.title.clone(), Some(sub.join(" · ")), false, width).on_press(move |s: &mut State| go_to(s, page.clone()));
    Some(section("sheet-release-sec", S::FromRelease.get(lang), r))
}

fn listeners(s: &State, width: f32) -> Option<El> {
    let n = s.detail.monthly_listeners?;
    let lang = s.lang;
    let mut c = col()
        .key("sheet-listeners")
        .w(width)
        .gap(6.0)
        .child(label(S::ListenersMonth.get(lang).to_owned(), th::BODY_M, th::text2()))
        .child(label(crate::i18n::group_digits(n), th::RELEASE, th::text()).key("sheet-listeners-n"));
    if let Some(d) = s.detail.listeners_delta.filter(|d| *d != 0) {
        let (ink, rest) = if d > 0 { (th::rgb(UP), S::ListenersMore) } else { (th::rgb(DOWN), S::ListenersFewer) };
        let num = crate::i18n::group_digits(d.unsigned_abs());
        let mut line = row().w(width).gap(4.0).flex_wrap();
        let by = S::ListenersBy.get(lang);
        if !by.is_empty() {
            line = line.child(label(by.to_owned(), th::BODY_M, th::text2()));
        }
        line = line.child(label(num, th::BODY_M, ink));
        for word in rest.get(lang).split(' ') {
            line = line.child(label(word.to_owned(), th::BODY_M, th::text2()));
        }
        c = c.child(line);
    }
    Some(c)
}

fn elsewhere(s: &State, width: f32) -> Option<El> {
    let lang = s.lang;
    let mut c = col().w(width).gap(14.0);
    let mut any = false;
    if let Some(link) = &s.detail.bandlink {
        let url = link.url.clone();
        let pic = glyph_disc(&icons::QUEUE, th::rgb(0xe8394a));
        c = c.child(lead_row("sheet-bandlink", pic, link.title.clone(), link.subtitle.clone(), true, width).help(link.url.as_str()).on_press_tx(
            move |_s: &mut State, tx: &mut Tx| {
                let _ = tx.open_url(&url);
            },
        ));
        any = true;
    }
    if !s.detail.links.is_empty() || s.detail.donation.is_some() {
        let mut pills = row().w(width).gap(8.0).flex_wrap();
        if let Some(url) = &s.detail.donation {
            pills = pills.child(link_pill("sheet-donate", &icons::HEART_FILL, S::SupportArtist.get(lang).to_owned(), url, true));
        }
        for (i, l) in s.detail.links.iter().enumerate() {
            let (ic, title) = link_badge(l, lang);
            pills = pills.child(link_pill(&format!("sheet-link-{i}"), ic, title, &l.href, false));
        }
        c = c.child(pills);
        any = true;
    }
    any.then(|| section("sheet-elsewhere", S::ArtistLinks.get(lang), c))
}

fn photos(s: &State, width: f32) -> Option<El> {
    let count = s.detail.photos.len().min(PHOTO_MAX);
    if count < 2 {
        return None;
    }
    let gap = 8.0;
    let cell = ((width - gap * (PHOTO_COLS as f32 - 1.0)) / PHOTO_COLS as f32).floor();
    let mut grid = row().key("sheet-photos").w(width).gap(gap).flex_wrap();
    for (i, tpl) in s.detail.photos.iter().take(count).enumerate() {
        grid = grid.child(el().key(format!("sheet-photo-{i}")).w(cell).h(cell).radius(12.0).child(kit::cover(s, Some(tpl.as_str()), cell, 12.0)));
    }
    Some(section("sheet-photos-sec", S::ArtistPhotos.get(s.lang), grid))
}

fn similar(s: &State, width: f32) -> Option<El> {
    if s.detail.similar.is_empty() {
        return None;
    }
    let cols = (((width + SIM_GAP) / (SIM_MIN_W + SIM_GAP)).floor() as usize).max(2);
    let cell = ((width - SIM_GAP * (cols as f32 - 1.0)) / cols as f32).floor();
    let mut grid = row().key("sheet-similar").w(width).gap(SIM_GAP).flex_wrap();
    for artist in s.detail.similar.iter().take(cols * 2) {
        let id = artist.id.clone();
        grid = grid.child(
            col()
                .key(format!("sheet-similar-{}", artist.id))
                .role("link")
                .label(artist.name.as_str())
                .w(cell)
                .h(SIM_H)
                .pt(2.0)
                .gap(10.0)
                .items_center()
                .radius(12.0)
                .cursor(Cursor::Pointer)
                .hov_group()
                .press_scale(th::PRESS)
                .on_press(move |s: &mut State| go_to(s, Page::Artist(id.clone())))
                .child(kit::cover(s, artist.cover_template(), SIM_PIC, SIM_PIC * 0.5))
                .child(label(artist.name.clone(), th::SMALL, th::text2()).gcol(th::text()).wrap().max_lines(2).align(Align::Center).w(cell - 6.0)),
        );
    }
    Some(section("sheet-similar-sec", S::SimilarArtists.get(s.lang), grid))
}

fn page_facts(s: &State, width: f32) -> Option<El> {
    let lang = s.lang;
    let d = &s.detail;
    let has_release_date = d.facts.iter().any(|(n, _)| *n == S::ReleaseDate);
    let mut facts: Vec<(&'static str, String)> = d
        .facts
        .iter()
        .filter(|(n, _)| !matches!(n, S::Description | S::SimilarArtists | S::MonthlyListeners) && !(has_release_date && *n == S::Year))
        .map(|(n, v)| (n.get(lang), v.clone()))
        .collect();
    if !d.labels.is_empty() {
        let name = if d.labels.len() > 1 { S::Labels } else { S::Label };
        facts.push((name.get(lang), d.labels.join(", ")));
    }
    if let Some(n) = d.likes {
        facts.push((S::LikesCount.get(lang), crate::i18n::group_digits(n)));
    }
    if facts.is_empty() {
        return None;
    }
    let gap = 16.0;
    let half = ((width - gap) * 0.5).floor();
    let grid = row().key("sheet-facts").w(width).gap(gap).flex_wrap().children(facts.into_iter().enumerate().map(|(i, (n, v))| fact_line(format!("sheet-fact-{i}"), n, v, half)));
    Some(section("sheet-facts-sec", S::Details.get(lang), grid))
}

fn about(s: &State, width: f32) -> Option<El> {
    let text = s.detail.facts.iter().find(|(n, _)| *n == S::Description).map(|(_, v)| v.clone())?;
    let name = if s.detail.is_artist { S::AboutArtist } else { S::Description };
    Some(section("sheet-about-sec", name.get(s.lang), label(text, th::LEAD, th::text2()).wrap().align(Align::Start).w(width).key("sheet-about")))
}

/// Builds the right-column sections in display order.
pub(super) fn sections(s: &State, what: &Sheet, width: f32) -> Vec<El> {
    let mut out = Vec::new();
    match what {
        Sheet::Track(t) => {
            out.push(credits(s, t, width));
            out.extend(release(s, t, width));
        }
        Sheet::Page if s.detail.is_artist => {
            out.extend(listeners(s, width));
            out.extend(about(s, width));
            out.extend(elsewhere(s, width));
            out.extend(photos(s, width));
            out.extend(similar(s, width));
        }
        Sheet::Page => {
            out.extend(page_facts(s, width));
            out.extend(about(s, width));
        }
    }
    out
}
