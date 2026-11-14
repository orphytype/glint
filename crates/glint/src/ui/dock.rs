//! The two pieces of the window that never move: the rail down the left side (the app's living
//! mark, the four places, settings and the profile) and the player bar along the bottom of the page.
//!
//! The bar is always there, so a page never shifts when music starts or stops: with a track it
//! holds the cover and names, the transport over a seek line that dances to the music, and the like,
//! lyrics, queue and volume buttons; with nothing playing it offers My Wave. The Now Playing page
//! carries its own player and covers both.

use rune::{Align, Cursor, Drag, El, Icon, Phase, Transition, Tx, col, el, icon, row};

use super::icons;
use super::kit::{self, BAR_GAP, BAR_H, BTN, GLYPH, RAIL_W, TITLE_H, label};
use crate::app::{NpTab, Page, Popover, Source, State};
use crate::fx::{self, Glass};
use crate::i18n::S;
use crate::store::{QualityPref, Repeat};
use crate::theme as th;

/// A place on the rail: its hit area and its glyph.
const NAV_BTN: f32 = 44.0;
const PINNED_BTN: f32 = 36.0;
const NAV_GLYPH: f32 = 22.0;
/// The round buttons of the window's chrome (back, close the player, the profile).
pub const CHIP: f32 = kit::CHROME;

// ---- the rail ----------------------------------------------------------------------------------------

pub fn rail(s: &State, w: f32) -> El {
    let items: [(Page, &'static Icon, &'static Icon, S); 3] = [
        (Page::Home, &icons::HOME, &icons::HOME_ON, S::Home),
        (Page::Search, &icons::SEARCH, &icons::SEARCH_ON, S::Search),
        (Page::Library, &icons::LIBRARY, &icons::LIBRARY_ON, S::Liked),
    ];
    let here = s.page.rail();
    let mut places = col().key("rail-places").w_full().gap(4.0).items_center();
    for (page, off, on_icon, name) in items {
        let on = here == page && s.page != Page::Settings;
        let target = page.clone();
        let mut b = row()
            .key(format!("nav-{}", page.key()))
            .role("button")
            .label(name.get(s.lang))
            .help(name.get(s.lang))
            .checked(on)
            .w(NAV_BTN)
            .h(NAV_BTN)
            .radius(14.0)
            .center()
            .cursor(Cursor::Pointer)
            .press_scale(th::PRESS)
            .hover_group()
            .on_press(move |s: &mut State| s.go(target.clone()))
            .child(kit::squircle_plate(NAV_BTN, NAV_BTN, kit::squircle_r(NAV_BTN), th::veil(0.06), None).group_opacity(0.0, 1.0));
        if on {
            // The marker glides from place to place.
            b = b.child(el().key("nav-mark").matched("nav-mark").absolute(-(RAIL_W - NAV_BTN) * 0.5 + 1.0, (NAV_BTN - 18.0) * 0.5).w(3.0).h(18.0).radius(1.5).fill(th::text()));
        }
        places = places.child(b.child(icon(if on { on_icon } else { off }, NAV_GLYPH).color(if on { th::text() } else { th::text3() })));
    }
    let pinned = pinned_strip(s);
    let downloads = row()
        .key("rail-downloads")
        .role("button")
        .label(S::DownloadManager.get(s.lang))
        .help(S::DownloadManager.get(s.lang))
        .w(NAV_BTN)
        .h(NAV_BTN)
        .radius(14.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .hover(th::veil(0.08))
        .on_press(|s: &mut State| s.open_popover(Popover::Downloads { anchor: "rail-downloads".into() }))
        .child(icon(&icons::DOWNLOAD, 19.0).color(if s.downloads.is_empty() { th::text3() } else { th::text2() }));
    let settings_on = s.page == Page::Settings;
    let settings = row()
        .key("rail-settings")
        .role("button")
        .label(S::Settings.get(s.lang))
        .help(S::Settings.get(s.lang))
        .checked(settings_on)
        .w(NAV_BTN)
        .h(NAV_BTN)
        .radius(14.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .hover_group()
        .on_press(|s: &mut State| s.go(Page::Settings))
        .child(kit::squircle_plate(NAV_BTN, NAV_BTN, kit::squircle_r(NAV_BTN), th::veil(if settings_on { 0.08 } else { 0.06 }), None).group_opacity(if settings_on { 1.0 } else { 0.0 }, 1.0))
        .child(icon(&icons::SETTINGS, NAV_GLYPH).color(if settings_on { th::text() } else { th::text3() }));
    // With a bar that leaves the rail's column free (a wide window), Settings sits level with the
    // middle of the bar; with one that runs under the rail, just above it.
    let bar = s.current().is_some();
    let level = bar && bar_span(w).0 >= kit::RAIL_W - 2.0;
    let foot = if level {
        BAR_GAP + (BAR_H - NAV_BTN) * 0.5
    } else if bar {
        BAR_H + 2.0 * BAR_GAP
    } else {
        20.0
    };
    col()
        .key("rail")
        .absolute(0.0, 0.0)
        .w(RAIL_W)
        .h_full()
        .items_center()
        .transition(Transition::none())
        .child(row().key("rail-top").w(RAIL_W).h(TITLE_H).center().window_drag().transition(Transition::none()).child(profile(s)))
        .child(places.mt(28.0))
        .child(pinned.mt(18.0))
        .child(el().grow(1.0))
        .child(downloads)
        .child(settings)
        .child(el().h(foot))
}

/// Cover-art shortcuts for pages the user pinned. Artists use circular frames while releases and
/// playlists keep their square cover shape. Each cover has the same secondary menu as a page
/// header, so opening and removing a pin never depends on the current tab.
fn pinned_strip(s: &State) -> El {
    let mut rail = col().key("pinned-items").gap(8.0).items_center();
    for item in &s.pinned_items {
        let item_key = item.key();
        let page = item.page.clone();
        let menu_item = item.clone();
        let radius = if item.artist { PINNED_BTN * 0.5 } else { 9.0 };
        let mut cover = row()
            .key(format!("pinned-{item_key}"))
            .role("button")
            .label(item.title.as_str())
            .help(item.title.as_str())
            .w(PINNED_BTN)
            .h(PINNED_BTN)
            .radius(radius)
            .clip_rounded(radius)
            .cursor(Cursor::Pointer)
            .press_scale(th::PRESS)
            .hover_group()
            .on_press(move |s: &mut State| s.go(page.clone()))
            .on_secondary_press(move |s: &mut State, _press, _tx: &mut Tx| {
                s.open_popover(Popover::Pinned { item: menu_item.clone(), anchor: format!("pinned-{item_key}") });
            })
            .child(kit::cover(s, item.cover.as_deref(), PINNED_BTN, radius));
        cover = cover.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::veil(0.0)).hover(th::veil(0.10)));
        rail = rail.child(cover);
    }
    rail
}

// ---- the profile -------------------------------------------------------------------------------------

/// The name the account goes by.
pub fn account_name(s: &State) -> String {
    match &s.account {
        Some(a) if !a.display_name.is_empty() => a.display_name.clone(),
        Some(a) => a.login.clone(),
        None => String::new(),
    }
}

fn initial(name: &str) -> String {
    name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()
}

/// A round button of the window's chrome, on glass (it sits over the page).
fn chip(key: &'static str, name: &str) -> El {
    row()
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .w(CHIP)
        .h(CHIP)
        .radius(CHIP * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .no_window_drag()
        .press_scale(th::PRESS)
        .transition(Transition::none())
        .child(kit::glass(Glass::Chip, CHIP * 0.5))
}

/// A glass chip with an icon in it.
#[allow(dead_code)]
pub fn icon_chip(key: &'static str, ic: &'static Icon, name: &str) -> El {
    chip(key, name).child(icon(ic, GLYPH).color(th::text()))
}

/// A way back or forward: a glass chip, faint and deaf while there is nowhere to go.
pub fn nav_chip(key: &'static str, ic: &'static Icon, name: &str, on: bool) -> El {
    let c = chip(key, name).child(icon(ic, GLYPH).color(if on { th::text() } else { th::text().alpha(0.28) }));
    if on { c } else { c.disabled(true).cursor(Cursor::Default) }
}

/// The account's picture, `size` points across (its initial until the picture is in, or when it
/// has none).
pub fn account_picture(s: &State, size: f32, initial: &str) -> El {
    let base = row().w(size).h(size).radius(size * 0.5).clip_rounded(size * 0.5).center().fill(th::veil(0.08)).transition(Transition::none());
    let style = if size >= 36.0 { th::H3 } else { th::BODY_M };
    let base = base.child(label(initial.to_owned(), style, th::text()));
    match &s.avatar {
        Some(url) => base.child(kit::picture_url(s, url)),
        None => base,
    }
}

/// The profile at the top of the rail: the account's picture; it opens the menu.
fn profile(s: &State) -> El {
    // Exactly the size of the back button beside it, on the same line; the ring that marks its
    // menu open is drawn around it, not into its size.
    row()
        .key("profile")
        .role("button")
        .label(S::Profile.get(s.lang))
        .help(S::Profile.get(s.lang))
        .w(CHIP)
        .h(CHIP)
        .radius(CHIP * 0.5)
        .center()
        .cursor(Cursor::Pointer)
        .no_window_drag()
        .press_scale(th::PRESS)
        .checked(s.menu_open)
        .hover_group()
        .on_press(|s: &mut State| {
            s.vol_open = false;
            s.menu_open = !s.menu_open;
        })
        .child(account_picture(s, CHIP, &initial(&account_name(s))))
        .child(el().absolute(0.0, 0.0).w(CHIP).h(CHIP).radius(CHIP * 0.5).fill(th::rgba(0xffffff, 0.10)).group_opacity(0.0, 1.0))
        .child(el().absolute(-3.0, -3.0).w(CHIP + 6.0).h(CHIP + 6.0).radius(CHIP * 0.5 + 3.0).border(1.5, th::veil(if s.menu_open { 0.55 } else { 0.0 })))
}

// ---- the player bar ----------------------------------------------------------------------------------

/// The bar along the bottom of the page, `w` being the window's width. It is there only while a
/// track is loaded: it rises from below the window when one starts and sinks away when the queue
/// is done. (Only an offset, no fade: the glass in it cannot take a group fade.)
///
/// In a window wider than the page's column ever gets (`kit::PAGE_MAX`), the bar stops growing
/// too: as wide as the column plus a little, centred over it.
pub fn bar(s: &State, w: f32) -> El {
    let (left, width) = bar_span(w);
    row()
        .key("bar")
        .pin(Some(left), None, Some(w - left - width), Some(BAR_GAP))
        .h(BAR_H)
        // Keep the glass slot outside a transition layer. RUNE skips backdrop effects inside a
        // moving layer, which made the dock blur disappear for one or more frames whenever a tab
        // remounted the page below it.
        .transition(Transition::none())
        // The bar is a surface of its own: a press or the pointer over it never reaches the page
        // underneath (its rows lit up and took clicks through it).
        .on_press(|_s: &mut State| {})
        .on_hover_change(|_s: &mut State, _on: bool, _tx: &mut Tx| {})
        .child(kit::glass(Glass::Dock, 22.0))
        .child(playing(s, width))
}

/// Where the bar starts and how wide it is in a window `w` wide.
pub fn bar_span(w: f32) -> (f32, f32) {
    let max = kit::PAGE_MAX - 2.0 * kit::PAGE_PAD + 2.0 * BAR_GAP + 32.0;
    if w - 2.0 * BAR_GAP <= max || w - kit::RAIL_W < kit::PAGE_MAX {
        return (BAR_GAP, w - 2.0 * BAR_GAP);
    }
    // Centred on the page's column, which is centred right of the rail.
    let centre = kit::RAIL_W + (w - kit::RAIL_W) * 0.5;
    (centre - max * 0.5, max)
}

fn playing(s: &State, width: f32) -> El {
    let Some(t) = s.current() else { return el() };
    let lang = s.lang;
    // Three columns: the track on the left, the transport in the middle, the rest on the right.
    // The middle keeps to the middle of the bar; the sides share what is left.
    let mid_w = (width * 0.36).clamp(280.0, 460.0);
    let side_w = ((width - mid_w) * 0.5 - 14.0).max(120.0);
    let compact = side_w < 230.0;
    // While My Wave plays, a line over the title says so, and which wave it is.
    let wave_from = s.playing_from.as_ref().filter(|f| f.wave && matches!(s.player.source, Source::Wave { .. })).map(|f| f.line(lang));

    let now = row()
        .key("bar-now")
        .role("button")
        .label(S::NowPlaying.get(lang))
        .w(side_w)
        .h(52.0)
        .gap(10.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .on_press(|s: &mut State| s.set_np(true))
        .on_secondary_press(|s: &mut State, press, _tx: &mut rune::Tx| s.open_player_menu(press.into()))
        // While the player is open (or coming over) the cover is up there, so it is not here: the
        // two share one id and the cover flies between them. (Two keys: the cover has to leave
        // the tree for the flight to start, not just lose its id.)
        .child(if s.np_open {
            el().key("bar-cover-gap").w(44.0).h(44.0)
        } else {
            kit::playing_cover(s, t, 44.0, 11.0, "bar-cover-morph", 3.0).key("bar-cover").matched("np-cover").motion(th::FLIGHT)
        })
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(if wave_from.is_some() { 1.0 } else { 2.0 })
                .children(wave_from.map(|text| {
                    kit::roll(
                        "bar-wave",
                        &text,
                        row()
                            .absolute(0.0, -12.0)
                            .gap(3.0)
                            .items_center()
                            .min_w(0.0)
                            .child(kit::wave_glyph(9.0, th::text2()))
                            .child(kit::caps(text.as_str(), th::text2()).truncate().min_w(0.0)),
                    )
                }))
                .child(kit::roll("bar-title", &t.full_title(), kit::track_title(s, t, "bar-title-link", th::H3, (side_w - 60.0).max(40.0))))
                .child(kit::roll("bar-artist", &t.artists_line(), kit::artist_line_width(s, t, th::SMALL, false, "bar-art", (side_w - 60.0).max(72.0)))),
        );

    let transport = row()
        .key("bar-transport")
        .h(32.0)
        .gap(4.0)
        .items_center()
        .justify_center()
        .child(shuffle_btn(s).w(30.0).h(30.0))
        .child(kit::ibtn("prev", &icons::PREV, 19.0, 32.0, th::text(), S::Previous.get(lang)).on_press(|s: &mut State| s.prev()))
        .child(kit::play_btn(s, 38.0, 24.0))
        .child(kit::ibtn("next", &icons::NEXT, 19.0, 32.0, th::text(), S::Next.get(lang)).on_press(|s: &mut State| s.next()))
        .child(repeat_btn(s).w(30.0).h(30.0));
    let seek = row()
        .key("bar-seek")
        .w_full()
        .h(16.0)
        .gap(8.0)
        .items_center()
        .child(label("0:00", th::MONO, th::text3()).live_text(&s.lives.clock).w(40.0).align(Align::End))
        .child(row().grow(1.0).min_w(0.0).child(seek_bars("seek", crate::ui::np::seed_of(&t.id), 18.0)))
        .child(label("-0:00", th::MONO, th::text3()).live_text(&s.lives.remaining).w(40.0));
    let middle = col().key("bar-mid").w(mid_w).gap(2.0).items_center().child(transport).child(seek);

    let bad = t.clone();
    let mut right = row().key("bar-right").w(side_w).h(56.0).gap(2.0).justify_end().items_center();
    if !compact {
        right = right.child(kit::ibtn("dock-dislike", &icons::DISLIKE, GLYPH, BTN, th::text2(), S::Dislike.get(lang)).on_press(move |s: &mut State| s.dislike(&bad)));
    }
    right = right.child(kit::like_button(s, t, BTN));
    if !compact {
        let lyrics = if s.lyrics_missing() {
            toggle_off("bar-lyrics", &icons::LYRICS, S::NoLyricsShort.get(lang))
        } else {
            toggle("bar-lyrics", &icons::LYRICS, false, S::Lyrics.get(lang)).on_press(|s: &mut State| {
                s.np_tab = NpTab::Lyrics;
                s.set_np(true);
            })
        };
        right = right
            .child(lyrics)
            .child(toggle("bar-queue", &icons::QUEUE, false, S::Queue.get(lang)).on_press(|s: &mut State| {
                s.np_tab = NpTab::Queue;
                s.set_np(true);
            }));
    }
    right = right.child(quality_btn(s)).child(volume_btn(s));

    row()
        .key("bar-playing")
        .w_full()
        .h_full()
        .px(10.0)
        .items_center()
        .justify_between()
        .transition(Transition::opacity().springs(th::RISE))
        .child(now)
        .child(middle)
        .child(right)
}

/// The speaker: its waves follow the level, and hovering or pressing it opens the volume card.
fn volume_btn(s: &State) -> El {
    let v = s.player.volume;
    let ic = if v < 0.01 {
        &icons::VOLUME_MUTE
    } else if v < 0.5 {
        &icons::VOLUME_LOW
    } else {
        &icons::VOLUME
    };
    let on = s.vol_open;
    let b = kit::ibtn("vol-btn", ic, GLYPH, BTN, th::text2(), S::Volume.get(s.lang)).checked(on);
    (if on { b.fill(th::veil(0.10)) } else { b })
        .on_hover_change(|s: &mut State, over: bool, _tx: &mut Tx| {
            if over {
                s.vol_open = true;
                s.vol_close_at = None;
            } else {
                s.vol_close_at = Some(s.now + 0.18);
            }
        })
        .on_press(|s: &mut State| {
        s.menu_open = false;
            s.vol_open = !s.vol_open;
            s.vol_close_at = None;
    })
}

fn quality_btn(s: &State) -> El {
    let (text, hint) = match s.settings.quality {
        QualityPref::Saver => (S::QualityShortSaver, S::QualitySaver),
        QualityPref::Balanced => (S::QualityShortBalanced, S::QualityBalanced),
        QualityPref::Best => (S::QualityShortBest, S::QualityBest),
    };
    row()
        .key("bar-quality")
        .role("button")
        .label(S::Quality.get(s.lang))
        .help(hint.get(s.lang))
        .h(30.0)
        .min_w(38.0)
        .px(9.0)
        .radius(15.0)
        .items_center()
        .justify_center()
        .cursor(Cursor::Pointer)
        .fill(th::veil(0.06))
        .hover(th::veil(0.10))
        .press(th::veil(0.14))
        .on_press(|s: &mut State| s.open_popover(Popover::Quality { anchor: "bar-quality".into() }))
        .child(label(text.get(s.lang), th::MONO, th::text2()))
}

/// Whether My Wave is what is playing.
/// Whether My Wave (one of its moods, not a wave seeded by a track or an artist) is playing.
pub fn wave_playing(s: &State) -> bool {
    matches!(&s.player.source, Source::Wave { preset, .. } if !preset.contains(':') && preset != "collection") && s.current().is_some()
}

// ---- shared with the player page ----------------------------------------------------------------------

/// A transport toggle (shuffle, repeat, lyrics, queue): full ink on a soft disc while on, dim
/// while off.
pub fn toggle(key: &str, ic: &'static Icon, on: bool, name: &str) -> El {
    let b = kit::ibtn(key.to_owned(), ic, GLYPH, BTN, if on { th::text() } else { th::text3() }, name).checked(on);
    if on { b.fill(th::veil(0.10)) } else { b }
}

/// A toggle that cannot be pressed now (no lyrics for this track): faint, no hover, `name` says why.
pub fn toggle_off(key: &str, ic: &'static Icon, name: &str) -> El {
    row()
        .key(key.to_owned())
        .role("button")
        .label(name)
        .help(name)
        .disabled(true)
        .w(BTN)
        .h(BTN)
        .radius(BTN * 0.5)
        .center()
        .child(icon(ic, GLYPH).color(th::veil(0.16)))
}

pub fn shuffle_btn(s: &State) -> El {
    toggle("shuffle", &icons::SHUFFLE, s.player.shuffle, S::Shuffle.get(s.lang)).on_press(|s: &mut State| s.toggle_shuffle())
}

pub fn repeat_btn(s: &State) -> El {
    let ic = if s.player.repeat == Repeat::One { &icons::REPEAT_ONE } else { &icons::REPEAT };
    toggle("repeat", ic, s.player.repeat != Repeat::Off, S::Repeat.get(s.lang)).on_press(|s: &mut State| s.cycle_repeat())
}

/// The seek line, drawn as bars (`fx::BARS`): the played ones lit, the others dim, both in the text
/// colour, and every bar along the row moving with the music. Dragging previews the position and
/// seeks on release.
pub fn seek_bars(key: &'static str, seed: f32, h: f32) -> El {
    let drag = |s: &mut State, d: &mut Drag, _tx: &mut Tx| {
        let f = if d.w > 0.0 { (d.x / d.w).clamp(0.0, 1.0) } else { 0.0 };
        let up = d.phase == Phase::Up;
        s.scrubbing = !up;
        s.player.pos_ms = (f as f64 * s.player.dur_ms as f64) as u64;
        s.sync_lives();
        if up {
            s.seek_frac(f);
        }
    };
    let (pitch, bar) = if h < 24.0 { (5.0, 2.5) } else { (6.0, 3.0) };
    el().key(key)
        .role("slider")
        .w_full()
        .h(h)
        .cursor(Cursor::Pointer)
        .transition(Transition::none())
        .on_drag(drag)
        .child(el().key(format!("{key}-bars")).absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).effect(fx::BARS).effect_data([seed, pitch, bar, 0.0]))
}
