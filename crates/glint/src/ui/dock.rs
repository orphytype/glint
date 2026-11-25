//! The two pieces of the window that never move: the rail down the left side (the app's living
//! mark, the four places, settings and the profile) and the player bar along the bottom of the page.
//!
//! The bar is always there, so a page never shifts when music starts or stops: with a track it
//! holds the cover and names, the transport over a seek line that dances to the music, and the like,
//! lyrics, queue and volume buttons; with nothing playing it offers My Wave. The Now Playing page
//! carries its own player and covers both.

use super::kit::Hover as _;
use crate::fx::Slot as _;
use rune::{Align, Cursor, Drag, El, Icon, Phase, Transition, Tx, col, el, icon, row};

use super::icons;
use super::kit::{self, BAR_GAP, BAR_H, RAIL_W, TITLE_H, label};
use super::np_icons as ni;
use crate::app::{NpTab, Page, Popover, Source, State};
use crate::fx::{self, Glass};
use crate::i18n::S;
use crate::store::{QualityPref, Repeat};
use crate::theme as th;

/// A place on the rail: its hit area and its glyph.
const NAV_BTN: f32 = 40.0;
const PINNED_BTN: f32 = 34.0;
const NAV_GLYPH: f32 = 20.0;
/// The two buttons at the rail's foot (downloads, settings): together as tall as the bar, so
/// they stand level with it.
const FOOT_BTN: f32 = BAR_H * 0.5;
/// The cover in the bar: nearly as tall as the bar, a little room above and below.
const BAR_COVER: f32 = 56.0;
/// The plate under a place on the rail: under the pointer, and (a little stronger) under the one open.
fn nav_plate(on: bool) -> El {
    el().absolute(0.0, 0.0).w_full().h_full().radius(10.0).fill(th::veil(if on { 0.08 } else { 0.05 })).gop(if on { 1.0 } else { 0.0 }, 1.0)
}
/// The round buttons of the window's chrome (back, close the player, the profile).
pub const CHIP: f32 = kit::CHROME;

// ---- the rail ----------------------------------------------------------------------------------------

pub fn rail(s: &State, w: f32, lead: f32, np_t: f32) -> El {
    let items: [(Page, &'static Icon, &'static Icon, S); 4] = [
        (Page::Search, &icons::SEARCH, &icons::SEARCH_ON, S::Search),
        (Page::Home, &icons::HOME, &icons::HOME_ON, S::Home),
        (Page::Explore, &icons::COMPASS, &icons::COMPASS_ON, S::Explore),
        (Page::Library, &icons::LIBRARY, &icons::LIBRARY_ON, S::Collection),
    ];
    let here = s.page.rail();
    let mut places = col().key("rail-places").w_full().gap(4.0).items_center();
    for (page, off, on_icon, name) in items {
        let on = here == page;
        let target = page.clone();
        let mut b = row()
            .key(format!("nav-{}", page.key()))
            .role("button")
            .label(name.get(s.lang))
            .help(name.get(s.lang))
            .checked(on)
            .w(NAV_BTN)
            .h(NAV_BTN)
            .radius(10.0)
            .center()
            .cursor(Cursor::Pointer)
            .press_scale(th::PRESS)
            .hov_group()
            .on_press(move |s: &mut State| {
                if target == Page::Library {
                    s.open_collection();
                } else {
                    s.go(target.clone());
                }
            });
        if on {
            // Both the indicator bar and active background plate glide smoothly between destinations.
            b = b
                .child(
                    el().key("nav-pill")
                        .matched("nav-pill")
                        .matched_in_place()
                        .animated_changes()
                        .motion(rune::Spring::new(0.24, 0.08))
                        .absolute(0.0, 0.0)
                        .w_full()
                        .h_full()
                        .radius(10.0)
                        .fill(th::veil(0.085)),
                )
                .child(
                    el().key("nav-mark")
                        .matched("nav-mark")
                        .animated_changes()
                        .motion(rune::Spring::new(0.24, 0.12))
                        .absolute(-(RAIL_W - NAV_BTN) * 0.5 + 1.0, (NAV_BTN - 18.0) * 0.5)
                        .w(3.0)
                        .h(18.0)
                        .radius(1.5)
                        .fill(th::text()),
                );
        } else {
            b = b.child(nav_plate(false));
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
        .h(FOOT_BTN)
        .radius(10.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .hov(th::veil(0.05))
        .on_press(|s: &mut State| s.open_popover(Popover::Downloads { anchor: "rail-downloads".into() }))
        .child(icon(&icons::DOWNLOAD, 18.0).color(if s.downloads.is_empty() { th::text3() } else { th::text2() }));
    let settings_on = s.settings_open;
    let settings = row()
        .key("rail-settings")
        .role("button")
        .label(S::Settings.get(s.lang))
        .help(S::Settings.get(s.lang))
        .checked(settings_on)
        .w(NAV_BTN)
        .h(FOOT_BTN)
        .radius(10.0)
        .center()
        .cursor(Cursor::Pointer)
        .press_scale(th::PRESS)
        .hov_group()
        .on_press(|s: &mut State| s.open_settings())
        .child(nav_plate(settings_on))
        .child(icon(&icons::SETTINGS, NAV_GLYPH).color(if settings_on { th::text() } else { th::text3() }));
    // Low in the rail, so the column's weight sits at its foot: with a bar that leaves the rail's
    // column free (a wide window), the two foot buttons stand level with the bar, top to bottom;
    // with one that runs under the rail, just above it.
    let bar = s.current().is_some() && s.page != Page::Home;
    let level = bar && bar_span(w).0 >= RAIL_W - 2.0;
    let foot = if level {
        BAR_GAP
    } else if bar {
        BAR_H + BAR_GAP + 8.0
    } else {
        14.0
    };
    let mac_chrome = cfg!(target_os = "macos") || lead > 0.0;
    let np_e = np_t.clamp(0.0, 1.0);
    let np_e = 1.0 - (1.0 - np_e).powi(3);
    let mut col_el = col()
        .key("rail")
        .absolute(0.0, 0.0)
        .w(RAIL_W)
        .h_full()
        .items_center()
        .offset(-16.0 * np_e, 0.0)
        .transition(Transition::none())
        .instant_changes();
    if mac_chrome {
        // On macOS the window's traffic-light buttons sit in the top-left corner over `rail-top`,
        // so the profile avatar moves right below them instead of overlapping the buttons.
        col_el = col_el
            .child(row().key("rail-top").w(RAIL_W).h(TITLE_H).window_drag().transition(Transition::none()))
            .child(profile(s).mt(2.0))
            .child(places.mt(10.0));
    } else {
        col_el = col_el
            .child(row().key("rail-top").w(RAIL_W).h(TITLE_H).center().window_drag().transition(Transition::none()).child(profile(s)))
            .child(places.mt(8.0));
    }
    col_el
        // The pins take the room between the places and the foot buttons and scroll inside it,
        // so many of them never run under the downloads and settings.
        .child(
            col()
                .key("rail-pins")
                .w(RAIL_W)
                .grow(1.0)
                .min_h(0.0)
                .mt(12.0)
                .mb(8.0)
                .scroll(rune::ScrollSpec { stick_bottom: false })
                .scroll_edge_fade(26.0)
                .child(col().w_full().items_center().py(6.0).child(pinned)),
        )
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
        let radius = if item.artist { PINNED_BTN * 0.5 } else { 8.0 };
        let mut cover = row()
            .key(format!("pinned-{item_key}"))
            .role("button")
            .label(item.title.as_str())
            .help(item.title.as_str())
            .w(PINNED_BTN)
            .h(PINNED_BTN)
            .radius(radius)
            .cursor(Cursor::Pointer)
            .press_scale(th::PRESS)
            .hov_group()
            .animated_changes()
            .motion(th::SLIDE)
            .transition(kit::pop_in(0.76, 0.0, th::SOFT))
            .on_press(move |s: &mut State| s.go(page.clone()))
            .on_secondary_press(move |s: &mut State, _press, _tx: &mut Tx| {
                s.open_popover(Popover::Pinned { item: menu_item.clone(), anchor: format!("pinned-{item_key}") });
            })
            .child(if item.page == Page::Library && item.cover.is_none() {
                row().w(PINNED_BTN).h(PINNED_BTN).radius(radius).center().fill(th::rgb(0x8e1920)).child(icon(&icons::HEART_FILL, 18.0).color(th::rgb(0xffb4a2)))
            } else {
                kit::cover_sm(s, item.cover.as_deref(), PINNED_BTN, radius)
            });
        cover = cover.child(el().absolute(0.0, 0.0).w_full().h_full().radius(radius).fill(th::veil(0.0)).hov(th::veil(0.10)));
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

/// Circular liquid-glass button in the title bar when a newer version is available or installing.
/// Pressing it opens the update summary popover.
pub fn update_pill(s: &State) -> Option<El> {
    use crate::update::Ui;
    let lang = s.lang;
    let tip = match &s.update {
        Ui::Available(r) => format!("{} {}", S::UpdateShort.get(lang), r.version.trim_start_matches('v')),
        Ui::Downloading { done, total, .. } => {
            let pct = if *total > 0 { (done * 100 / total).min(100) } else { 0 };
            format!("{} {pct}%", S::UpdateDownloading.get(lang))
        }
        Ui::Installing => S::UpdateInstalling.get(lang).to_owned(),
        _ => return None,
    };
    let open = s.popover_open && matches!(s.popover, Some(Popover::Update { .. }));
    let glyph = if matches!(s.update, Ui::Downloading { .. } | Ui::Installing) {
        kit::spin(icon(&icons::SPINNER, 18.0).color(th::text()))
    } else {
        icon(&icons::UPDATE_RING, 19.0).color(th::text())
    };
    Some(chip("update-pill", &tip).checked(open).hov(th::veil(0.06)).child(glyph).on_press(|s: &mut State| s.open_popover(Popover::Update { anchor: "update-pill".into() })))
}

/// A way back or forward: a compact pill in the title bar, subtle when disabled.
/// When `flat` is true (Home's full-bleed fluid stage), uses a lightweight frosted pill instead of
/// `Glass::Chip` so Home never allocates a full-window backdrop copy just for two 30pt buttons.
pub fn nav_chip(key: &'static str, ic: &'static Icon, name: &str, on: bool, flat: bool) -> El {
    const SIZE: f32 = 30.0;
    let base = row()
        .key(key)
        .role("button")
        .label(name)
        .help(name)
        .w(SIZE)
        .h(SIZE)
        .radius(SIZE * 0.5)
        .center()
        .no_window_drag()
        .transition(Transition::none());
    if on {
        let b = base.cursor(Cursor::Pointer).press_scale(th::PRESS).hov(th::veil(0.08));
        let b = if flat {
            b.fill(th::veil(if th::light() { 0.075 } else { 0.085 })).border(1.0, th::veil(if th::light() { 0.10 } else { 0.12 }))
        } else {
            b.child(kit::glass(Glass::Chip, SIZE * 0.5))
        };
        b.child(icon(ic, 15.0).color(th::text()))
    } else {
        base.disabled(true)
            .cursor(Cursor::Default)
            .fill(th::veil(0.035))
            .border(1.0, th::veil(0.04))
            .child(icon(ic, 15.0).color(th::text().alpha(0.24)))
    }
}

/// The account's picture, `size` points across (its initial until the picture is in, or when it
/// has none).
pub fn account_picture(s: &State, size: f32, initial: &str) -> El {
    let base = row().w(size).h(size).radius(size * 0.5).center().fill(th::veil(0.08)).transition(Transition::none());
    let style = if size >= 36.0 { th::H3 } else { th::BODY_M };
    let base = base.child(label(initial.to_owned(), style, th::text()));
    match &s.avatar {
        Some(url) => base.child(kit::picture_url(s, url, size * 0.5)),
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
        .hov_group()
        .on_press(|s: &mut State| {
            s.close_volume();
            s.menu_open = !s.menu_open;
        })
        .child(account_picture(s, CHIP, &initial(&account_name(s))))
        .child(el().absolute(0.0, 0.0).w(CHIP).h(CHIP).radius(CHIP * 0.5).fill(th::rgba(0xffffff, 0.10)).gop(0.0, 1.0))
        .child(el().absolute(-3.0, -3.0).w(CHIP + 6.0).h(CHIP + 6.0).radius(CHIP * 0.5 + 3.0).border(1.5, th::veil(if s.menu_open { 0.55 } else { 0.0 })))
}

// ---- the player bar ----------------------------------------------------------------------------------

/// The bar along the bottom of the page, `w` being the window's width. It is there only while a
/// track is loaded: it rises from below the window when one starts and sinks away when the queue
/// is done. (Only an offset, no fade: the glass in it cannot take a group fade.)
///
/// In a window wider than the page's column ever gets (`kit::PAGE_MAX`), the bar stops growing
/// too: as wide as the column plus a little, centred over it.
pub fn bar(s: &State, w: f32, np_open: bool, np_t: f32) -> El {
    let (left, width) = bar_span(w);
    let smooth = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let bar_e = if s.headless && s.bar_pop.t == 0.0 && s.page != Page::Home && s.current().is_some() { 1.0 } else { smooth(s.bar_pop.t) };
    let np_out = if np_open { smooth(np_t / 0.55) } else { smooth((np_t - 0.12) / 0.65) };
    let vis = (bar_e * (1.0 - np_out)).clamp(0.0, 1.0);
    // Slide down when opening Now Playing or leaving to Home; keep y=0 while closing Now Playing
    // so `matched("np-cover")` lands pixel-dead-center on `bar-cover` with zero micro-snap.
    let dy = (1.0 - bar_e) * (BAR_H + BAR_GAP + 8.0) + if np_open { 38.0 * np_out } else { 0.0 };
    row()
        .key("bar")
        .pin(Some(left), None, Some(w - left - width), Some(BAR_GAP))
        .h(BAR_H)
        .offset(0.0, dy)
        // Keep the glass slot outside a transition layer. RUNE skips backdrop effects inside a
        // moving layer, which made the dock blur disappear for one or more frames whenever a tab
        // remounted the page below it.
        .transition(Transition::none())
        .instant_changes()
        // The bar is a surface of its own: a press or the pointer over it never reaches the page
        // underneath, and while the pointer is on it the page takes no hover (`kit::set_under`).
        .on_press(|_s: &mut State| {})
        .on_hover_change(|s: &mut State, on: bool, _tx: &mut Tx| s.over_bar = on)
        .child(kit::glass_alpha(Glass::Dock, 18.0, vis))
        .child(playing(s, width, vis))
}

/// Where the bar starts and how wide it is in a window `w` wide.
pub fn bar_span(w: f32) -> (f32, f32) {
    let max = kit::PAGE_MAX - 2.0 * kit::PAGE_PAD + 2.0 * BAR_GAP + 32.0;
    if w - 2.0 * BAR_GAP <= max || w - RAIL_W < kit::PAGE_MAX {
        return (BAR_GAP, w - 2.0 * BAR_GAP);
    }
    // Centred on the page's column, which is centred right of the rail.
    let centre = RAIL_W + (w - RAIL_W) * 0.5;
    (centre - max * 0.5, max)
}

fn playing(s: &State, width: f32, vis: f32) -> El {
    let Some(t) = s.current() else { return el() };
    let lang = s.lang;
    // Three columns: the track on the left, the transport in the middle, the rest on the right.
    // The middle keeps to the middle of the bar; the sides share what is left. A narrow bar packs
    // the transport closer, so all seven buttons stay.
    let narrow = width < 900.0;
    let mid_w = if narrow { (width * 0.34).clamp(250.0, 300.0) } else { (width * 0.36).clamp(280.0, 460.0) };
    let side_w = ((width - mid_w) * 0.5 - 14.0).max(120.0);
    let can_match = !fx::cpu_mode() && s.page != Page::Home;

    let mut now_text = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .offset(0.0, (1.0 - vis) * 8.0)
        .child(kit::roll("bar-title", &t.full_title(), kit::track_title(s, t, "bar-title-link", th::H3, (side_w - BAR_COVER - 16.0).max(40.0))))
        .child(kit::roll("bar-artist", &t.artists_line(), kit::artist_line_width(s, t, th::SMALL, false, "bar-art", (side_w - BAR_COVER - 16.0).max(72.0))));
    if vis < 0.999 {
        now_text = now_text.opacity(vis);
    }

    let now = row()
        .key("bar-now")
        .role("button")
        .label(S::NowPlaying.get(lang))
        .w(side_w)
        .h(BAR_COVER)
        .gap(10.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .on_press(|s: &mut State| s.set_np(true))
        .on_secondary_press(|s: &mut State, press, _tx: &mut Tx| s.open_player_menu(press.into()))
        // While the player is open (or coming over) the cover is up there, so it is not here: the
        // two share one id and the cover flies between them. (Two keys: the cover has to leave
        // the tree for the flight to start, not just lose its id.)
        .child(if s.np_open {
            el().key("bar-cover-gap").w(BAR_COVER).h(BAR_COVER)
        } else {
            let cov = kit::playing_cover(s, t, BAR_COVER, 10.0, "bar-cover-mark").key("bar-cover");
            if can_match { cov.matched("np-cover").animated_changes().motion(th::FLIGHT) } else { cov }
        })
        .child(now_text);

    // Not this one and liked at the two ends, shuffle and repeat inside them: as in the open player.
    let bad = t.clone();
    let mut transport = row()
        .key("bar-transport")
        .h(32.0)
        .gap(if narrow { 2.0 } else { 6.0 })
        .items_center()
        .justify_center()
        .offset(0.0, (1.0 - vis) * 8.0)
        .child(kit::np_like_button(s, t, 32.0, 18.0))
        .child(shuffle_btn(s))
        .child(kit::skip_btn(s, false, "prev", &ni::PREV, 18.0, 32.0, th::text2()))
        .child(kit::bar_play_btn(s, 36.0, 22.0))
        .child(kit::skip_btn(s, true, "next", &ni::NEXT, 18.0, 32.0, th::text2()))
        .child(repeat_btn(s))
        .child(kit::quiet_btn("dock-dislike", &ni::DISLIKE, 18.0, 32.0, th::text3(), S::Dislike.get(lang)).on_press(move |s: &mut State| s.dislike(&bad)));
    if vis < 0.999 {
        transport = transport.opacity(vis);
    }
    let time_w = if s.player.dur_ms >= 3_600_000 { 58.0 } else { 40.0 };
    let seek_w = (mid_w - 2.0 * time_w - 16.0).max(40.0);
    let seek = row()
        .key("bar-seek")
        .w_full()
        .h(16.0)
        .gap(8.0)
        .items_center()
        .offset(0.0, (1.0 - vis) * 8.0)
        .child(label("0:00", th::MONO, th::text3().alpha(vis)).live_text(&s.lives.clock).w(time_w).align(Align::End))
        .child(row().grow(1.0).min_w(0.0).child(seek_bars_alpha(s, "seek", crate::ui::np::seed_of(&t.id), seek_w, 18.0, true, vis)))
        .child(label("-0:00", th::MONO, th::text3().alpha(vis)).live_text(&s.lives.remaining).w(time_w));
    let middle = col().key("bar-mid").w(mid_w).gap(2.0).items_center().child(transport).child(seek);

    // The right side takes what fits, most needed first: More and the volume always, then the
    // quality (its pill, or just its icon), the queue, the lyrics, the mini player, and last the
    // volume slider beside the speaker. They stay in one order on screen.
    let rbtn = RIGHT_BTN;
    let pill_text = bar_quality_text(s);
    let pill_w = kit::measure_w(&pill_text, BADGE_STYLE).ceil() + 2.0 * 8.0 + 2.0 * 2.0 + 2.0;
    let mut room = side_w - 2.0 * (rbtn + RIGHT_GAP);
    let mut take = |w: f32| {
        let fits = room >= w + RIGHT_GAP;
        if fits {
            room -= w + RIGHT_GAP;
        }
        fits
    };
    let pill = take(pill_w);
    let quality_icon = !pill && take(rbtn);
    let queue_on = take(rbtn);
    let lyrics_on = queue_on && take(rbtn);
    let mini_on = lyrics_on && take(rbtn);
    let inline_vol = mini_on && take(VOL_SLIDER_W);

    let mut right = row().key("bar-right").w(side_w).h(56.0).gap(RIGHT_GAP).justify_end().items_center();
    if lyrics_on {
        right = right.child(if s.lyrics_missing() {
            kit::quiet_off("bar-lyrics", &ni::LYRICS, 18.0, rbtn, S::NoLyricsShort.get(lang))
        } else {
            kit::quiet_btn("bar-lyrics", &ni::LYRICS, 18.0, rbtn, th::text2(), S::Lyrics.get(lang)).on_press(|s: &mut State| {
                s.set_np_tab(NpTab::Lyrics);
                s.set_np(true);
            })
        });
    }
    if queue_on {
        right = right.child(kit::quiet_btn("bar-queue", &ni::QUEUE, 18.0, rbtn, th::text2(), S::Queue.get(lang)).on_press(|s: &mut State| {
            s.set_np_tab(NpTab::Queue);
            s.set_np(true);
        }));
    }
    if pill {
        right = right.child(quality_pill(s, "bar-quality", pill_text, 24.0).mx(2.0));
    } else if quality_icon {
        right = right.child(quality_btn_at(s, "bar-quality", rbtn, th::text2()));
    }
    // With room, a slider beside the speaker (which mutes); without, the speaker opens the volume
    // card on hover and mutes on click.
    right = if inline_vol { right.child(volume_inline(s)) } else { right.child(volume_btn(s, rbtn)) };
    if mini_on {
        right = right.child(
            kit::quiet_toggle("bar-mini", &ni::MINI, 18.0, rbtn, s.mini_open, S::MiniPlayer.get(lang)).on_press(|s: &mut State| s.toggle_mini_player()),
        );
    }
    // What can be done with the track (as the right click on it, or the open player's "more").
    right = right.child(
        kit::quiet_btn("bar-more", &ni::MORE, 18.0, rbtn, th::text2(), S::More.get(lang)).on_press(|s: &mut State| s.open_player_menu(rune::PopupAnchor::Key("bar-more".into()))),
    );
    right = right.offset(0.0, (1.0 - vis) * 8.0);
    if vis < 0.999 {
        right = right.opacity(vis);
    }

    row()
        .key("bar-playing")
        .w_full()
        .h_full()
        .pl((BAR_H - BAR_COVER) * 0.5)
        .pr(10.0)
        .items_center()
        .justify_between()
        .transition(Transition::none())
        .child(now)
        .child(middle)
        .child(right)
}

/// Nudges volume from a mouse wheel or trackpad event.
pub(super) fn volume_wheel(s: &mut State, e: rune::WheelEvent, _tx: &mut Tx) -> bool {
    if e.dy == 0.0 {
        return false;
    }
    let step = if e.precise { -e.dy * 0.004 } else { -e.dy.signum() * 0.05 };
    s.set_volume((s.player.volume + step).clamp(0.0, 1.0));
    true
}

/// Modern horizontal volume slider shared by the mini-player bar and Now Playing.
pub(super) fn volume_slider(s: &State, key: &'static str, w: f32) -> El {
    let v = s.player.volume.clamp(0.0, 1.0);
    let fill_w = if v > 0.005 { (w * v).max(4.0) } else { 0.0 };
    let thumb_x = (w * v - 6.0).clamp(0.0, (w - 12.0).max(0.0));
    let vol_spring = rune::Spring::new(0.11, 0.0);
    el().key(key)
        .role("slider")
        .label(S::Volume.get(s.lang))
        .w(w)
        .h(28.0)
        .cursor(Cursor::Pointer)
        .hov_group()
        .on_wheel(volume_wheel)
        .on_drag(|s: &mut State, d: &mut Drag, _tx: &mut Tx| {
            let f = if d.w > 0.0 { (d.x / d.w).clamp(0.0, 1.0) } else { 0.0 };
            s.set_volume(f);
        })
        .child(el().absolute(0.0, 12.0).w(w).h(4.0).radius(2.0).fill(th::veil(0.16)))
        .child(el().key(format!("{key}-fill")).absolute(0.0, 12.0).w(fill_w).h(4.0).radius(2.0).fill(th::text2()).gcol(th::text()).animated_changes().motion(vol_spring))
        .child(el().key(format!("{key}-thumb")).absolute(thumb_x, 8.0).w(12.0).h(12.0).radius(6.0).fill(th::text()).gop(0.0, 1.0).animated_changes().motion(vol_spring))
}

/// The speaker and a slider beside it: pressing the speaker mutes (and brings the volume back),
/// dragging or pressing the slider sets it, the wheel over either nudges it.
fn volume_inline(s: &State) -> El {
    let v = s.player.volume.clamp(0.0, 1.0);
    let ic = if v < 0.01 {
        &ni::VOLUME_MUTE
    } else if v < 0.5 {
        &ni::VOLUME_LOW
    } else {
        &ni::VOLUME
    };
    let speaker = kit::quiet_btn("vol-mute", ic, 18.0, RIGHT_BTN, th::text2(), S::Volume.get(s.lang)).on_wheel(volume_wheel).on_press(State::toggle_mute);
    row().key("vol-inline-row").gap(2.0).pr(4.0).items_center().child(speaker).child(volume_slider(s, "vol-inline", 60.0))
}

/// The bar's buttons on the right, and the room between them.
const RIGHT_BTN: f32 = 32.0;
const RIGHT_GAP: f32 = 2.0;
/// What the inline volume adds to the speaker: the slider and its padding.
const VOL_SLIDER_W: f32 = 60.0 + 2.0 + 4.0;

/// The speaker opens the volume card on hover, adjusts volume on wheel, and mutes on click.
fn volume_btn(s: &State, size: f32) -> El {
    let v = s.player.volume;
    let ic = if v < 0.01 {
        &ni::VOLUME_MUTE
    } else if v < 0.5 {
        &ni::VOLUME_LOW
    } else {
        &ni::VOLUME
    };
    let on = s.vol_open;
    let b = kit::ibtn("vol-btn", ic, 18.0, size, th::text2(), S::Volume.get(s.lang)).checked(on);
    (if on { b.fill(th::veil(0.10)) } else { b })
        .on_wheel(volume_wheel)
        .on_hover_change(|s: &mut State, over: bool, _tx: &mut Tx| {
            s.vol_button_hover = over;
            if over {
                s.vol_open = true;
                s.vol_close_at = None;
            } else {
                s.vol_close_at = Some(s.now + 0.28);
            }
        })
        .on_press(State::toggle_mute)
}

const BADGE_STYLE: rune::TextStyle = rune::TextStyle::new(11.0, 500.0).family(th::SANS).tracking(0.15).lh(14.0).features("'tnum' 1");

/// Cached maximum audio quality offered by Yandex for the currently playing track.
/// Prefers the higher of the probed maximum and the live playing stream so lossless streams
/// never display a stale lower rate.
pub(super) fn current_track_max_quality(s: &State) -> Option<crate::net::StreamQuality> {
    let probed = s.current().and_then(|t| s.track_max_quality.get(&t.id).copied());
    let live = s.player.current_audio;
    match (probed, live) {
        (Some(p), Some(l)) if l.is_higher_than(p) => Some(l),
        (Some(p), _) => Some(p),
        (None, Some(l)) if s.settings.quality == QualityPref::Best => Some(l),
        _ => None,
    }
}

fn quality_tip(s: &State) -> String {
    let lang = s.lang;
    let base = format!("{} · {}", S::Quality.get(lang), ni::quality_label(s.settings.quality).get(lang));
    let cur = s.player.current_audio;
    let max = current_track_max_quality(s);
    match (cur, max) {
        (Some(c), Some(m)) if m.is_higher_than(c) => {
            format!("{base} · {}: {} · {}: {}", S::QualityNow.get(lang), c.format_full(lang), S::QualityMaxForTrack.get(lang), m.format_full(lang))
        }
        (Some(c), _) => {
            let fmt = c.format_full(lang);
            if fmt.is_empty() { base } else { format!("{base} · {fmt}") }
        }
        (None, Some(m)) => {
            let fmt = m.format_full(lang);
            if fmt.is_empty() { base } else { format!("{base} · {}: {fmt}", S::QualityMaxForTrack.get(lang)) }
        }
        (None, None) => base,
    }
}

/// What the quality menu and the bar plate show for a tier: its bitrate, capped by the track.
pub(crate) fn quality_rate_text(quality: QualityPref, max: Option<crate::net::StreamQuality>, lang: crate::i18n::Lang, compact: bool) -> String {
    let kbps = S::Kbps.get(lang);
    match quality {
        QualityPref::Saver => {
            let rate = max.map(|m| m.bitrate_kbps).filter(|&b| b > 0 && b < 64).unwrap_or(64);
            format!("{rate} {kbps}")
        }
        QualityPref::Balanced => {
            let rate = max.map(|m| m.bitrate_kbps).filter(|&b| b > 0 && b < 192).unwrap_or(192);
            format!("{rate} {kbps}")
        }
        QualityPref::Best => {
            let text = if compact { max.map(|m| m.format_compact(lang)) } else { max.map(|m| m.format_full(lang)) };
            text.filter(|t| !t.is_empty()).unwrap_or_else(|| S::QualityMax.get(lang).to_owned())
        }
    }
}

/// The plate in the bar shows the quality the listener picked, not the stream still playing.
fn bar_quality_text(s: &State) -> String {
    quality_rate_text(s.settings.quality, current_track_max_quality(s), s.lang, true)
}

fn quality_pill(s: &State, key: &'static str, text: String, h: f32) -> El {
    let open = s.popover_open && matches!(&s.popover, Some(Popover::Quality { anchor }) if anchor == key);
    let tip = quality_tip(s);
    row()
        .key(key)
        .role("button")
        .label(tip.as_str())
        .help(tip)
        .checked(open)
        .h(h)
        .px(8.0)
        .radius(h * 0.5)
        .items_center()
        .cursor(Cursor::Pointer)
        .fill(th::veil(if open { 0.12 } else { 0.06 }))
        .border(1.0, th::veil(if open { 0.16 } else { 0.08 }))
        .hov(th::veil(0.06))
        .press(th::veil(0.10))
        .press_scale(th::PRESS)
        .on_press(move |s: &mut State| s.open_popover(Popover::Quality { anchor: key.into() }))
        .child(label(text, BADGE_STYLE, if open { th::text() } else { th::text2() }))
}

pub(super) fn quality_btn_at(s: &State, key: &'static str, size: f32, rest: rune::Color) -> El {
    let q = s.settings.quality;
    let tip = quality_tip(s);
    let open = s.popover_open && matches!(&s.popover, Some(Popover::Quality { anchor }) if anchor == key);
    let ic = ni::quality_icon(q);
    let b = if open { kit::quiet_toggle(key, ic, 18.0, size, true, &tip) } else { kit::quiet_btn(key, ic, 18.0, size, rest, &tip) };
    b.on_press(move |s: &mut State| s.open_popover(Popover::Quality { anchor: key.into() }))
}

/// Whether My Wave (one of the Home arc wheel waves, not a standalone artist/track/collection wave) is playing.
pub fn wave_playing(s: &State) -> bool {
    s.current().is_some()
        && matches!(&s.player.source, Source::Wave { preset, .. } if (!preset.contains(':') && preset != "collection") || s.wheel_wave(preset).is_some())
}

// ---- shared with the player page ----------------------------------------------------------------------

pub fn shuffle_btn(s: &State) -> El {
    kit::quiet_toggle("shuffle", &ni::SHUFFLE, 18.0, 32.0, s.player.shuffle, S::Shuffle.get(s.lang)).on_press(|s: &mut State| s.toggle_shuffle())
}

pub fn repeat_btn(s: &State) -> El {
    let ic = if s.player.repeat == Repeat::One { &ni::REPEAT_ONE } else { &ni::REPEAT };
    kit::quiet_toggle("repeat", ic, 18.0, 32.0, s.player.repeat != Repeat::Off, S::Repeat.get(s.lang)).on_press(|s: &mut State| s.cycle_repeat())
}

/// The seek line: drawn with `fx::BARS` once its shader is ready, or as a lightweight native RUNE
/// progress bar driven by a `Live<[f32; 4]>` handle (zero view rebuilds) when shaders are off.
pub fn seek_bars_alpha(s: &State, key: &'static str, seed: f32, width: f32, h: f32, dock: bool, alpha: f32) -> El {
    let a = alpha.clamp(0.0, 1.0);
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
    let slot = el().key(key).role("slider").w_full().h(h).cursor(Cursor::Pointer).transition(Transition::none()).on_drag(drag);
    if s.settings.stage.waveform && fx::bars_ready() {
        let (pitch, bar) = if h < 24.0 { (5.0, 2.5) } else { (6.0, 3.0) };
        return slot.child(el().key(format!("{key}-bars")).absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).fx(fx::BARS).fx_data([seed, pitch, bar, 1.0 - a]));
    }
    let w = width.max(40.0);
    let frac = if s.player.dur_ms > 0 { (s.player.pos_ms as f32 / s.player.dur_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
    let (live, thumb_live) = s.update_seek_live(dock, w, frac);
    let band_top = ((h - 12.0) * 0.5).max(0.0);
    let thumb_rest = if s.scrubbing { 1.0 } else { 0.0 };
    slot.hov_group().child(
        el().key(format!("{key}-band"))
            .absolute(0.0, band_top)
            .w(w)
            .h(12.0)
            .transition(Transition::none())
            .child(
                el().key(format!("{key}-track"))
                    .absolute(0.0, 4.0)
                    .w(w)
                    .h(4.0)
                    .radius(2.0)
                    .fill(th::veil(0.18 * a))
                    .hov(th::veil(0.08 * a))
                    .transition(Transition::none())
                    .child(el().key(format!("{key}-fill")).radius(2.0).fill(th::text().alpha(a)).live_rect(live).transition(Transition::none())),
            )
            .child(el().key(format!("{key}-thumb")).radius(6.0).fill(th::text().alpha(a)).live_rect(thumb_live).gop(thumb_rest, 1.0).transition(Transition::none())),
    )
}
