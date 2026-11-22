//! Home page: time-of-day greeting, My Wave garden, and catalog shelves.

use rune::{Cursor, El, Transition, col, el, icon, row};
use ym_api::{Shelf, ShelfItem, ShelfKind};

use super::garden::{bug_layer, garden};
use super::shared::{CARD_GAP, CARD_R, GRID_GAP, album_card, column, fit, hscroll, playlist_card, row_arrows, start_wave_with, track_card, wave_title};
use crate::app::{Load, State};
use crate::i18n::{Lang, S};
use crate::theme as th;
use crate::ui::blobs::{self, Family, Fill, Look, Rng};
use crate::ui::{
    dock, icons,
    kit::{self, Hover as _, label},
};

pub(super) fn home(s: &State, w: f32, _h: f32) -> El {
    let cw = column(w);
    let compact = cw < 840.0;
    let mut top =
        col().key("home-top").w_full().gap(if compact { 24.0 } else { 40.0 }).transition(Transition::none()).child(kit::rise(greeting(s, compact), 0)).child(garden(s, cw));
    if s.bugs_alive() && s.swarm.seed == s.home_seed && s.np.t <= 0.0 {
        top = top.child(bug_layer(s));
    }
    let mut body = col().gap(if compact { 32.0 } else { 40.0 }).child(top);
    match &s.home {
        Load::Ready(shelves) => {
            let mut order: Vec<usize> = (0..shelves.len()).collect();
            order.sort_by_key(|&i| (std::cmp::Reverse(shelves[i].items.len()), shelves[i].kind != ShelfKind::NewReleases));
            let mut shown = 0;
            for (i, sh) in order.into_iter().map(|i| (i, &shelves[i])) {
                if sh.items.is_empty() {
                    continue;
                }
                shown += 1;
                body = body.child(
                    shelf(s, "", i, sh, w)
                        .transition(Transition::asymmetric(Transition::offset(0.0, 10.0), Transition::none()).springs(th::RISE))
                        .enter_delay(0.05 * shown.min(4) as f32),
                );
            }
        }
        Load::Failed(_) => {
            if let Some(e) = kit::list_state(&s.home, s) {
                body = body.child(e);
            }
        }
        _ => {
            for i in 0..2 {
                body = body.child(shelf_bones(w, i));
            }
        }
    }
    kit::scroll_page_live("home", body)
}

fn greeting(s: &State, compact: bool) -> El {
    let lang = s.lang;
    let hour = crate::app::local_hour();
    let (hello, lead, mood) = match hour {
        5..=11 => (S::GoodMorning, S::SuggestMorning, "energetic"),
        12..=17 => (S::GoodAfternoon, S::SuggestDay, "discover"),
        18..=23 => (S::GoodEvening, S::SuggestEvening, "calm"),
        _ => (S::GoodNight, S::SuggestNight, "sad"),
    };
    let (open, close) = if lang == Lang::Ru { ("«", "»") } else { ("\u{201c}", "\u{201d}") };
    let playing_it = dock::wave_playing(s) && s.wave_choice == mood;
    let hint = format!("{} {open}{}{close}", lead.get(lang), wave_title(mood).get(lang));
    let suggest_fade = rune::Spring::new(0.3, 0.0);
    let suggest_icon = if playing_it {
        icon(&icons::PLAY, 11.0).color(th::text2()).opacity(0.0).motion(suggest_fade)
    } else {
        icon(&icons::PLAY, 11.0).color(th::text2()).gop(0.5, 1.0).motion(suggest_fade)
    };
    let suggest = row()
        .key("home-suggest")
        .role("button")
        .label(&hint)
        .disabled(playing_it)
        .gap(6.0)
        .items_center()
        .cursor(if playing_it { Cursor::Default } else { Cursor::Pointer })
        .hov_group()
        .press_scale(0.98)
        .on_press(move |s: &mut State| start_wave_with(s, mood))
        .child(label(hint, th::BODY, th::text2()).opacity(if playing_it { 0.0 } else { 1.0 }).motion(suggest_fade))
        .child(suggest_icon);
    let name = dock::account_name(s);
    let first = name.split_whitespace().next().unwrap_or("");
    let title = if first.is_empty() { hello.get(lang).to_owned() } else { format!("{}, {first}", hello.get(lang)) };
    row()
        .key("home-head")
        .w_full()
        .items_center()
        .gap(16.0)
        .mt(if compact { 10.0 } else { 20.0 })
        .child(col().grow(1.0).min_w(0.0).gap(6.0).child(row().key("home-title").child(label(title, th::TITLE, th::text()).max_lines(2))).child(suggest))
        .child(
            row()
                .key("home-reroll")
                .role("button")
                .label(S::RerollShapes.get(lang))
                .help(S::RerollShapes.get(lang))
                .w(dock::CHIP)
                .h(dock::CHIP)
                .radius(dock::CHIP * 0.5)
                .center()
                .cursor(Cursor::Pointer)
                .press_scale(th::PRESS)
                .hov(th::veil(0.06))
                .child(kit::glass(crate::fx::Glass::Chip, dock::CHIP * 0.5))
                .child(icon(&icons::SPARKLE, 18.0).color(th::text()))
                .on_press(|s: &mut State| s.home_seed = s.home_seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)),
        )
}

fn shelf_bones(w: f32, i: usize) -> El {
    let (_, size) = fit(w, 144.0, GRID_GAP, 7);
    let size = size.min(200.0);
    let n = (((column(w) + CARD_GAP) / (size + CARD_GAP)).floor() as usize).max(1);
    let d = 2.0 * blobs::radius(Family::Heart, 36.0);
    let mark = el().w(36.0).h(36.0).center().child(el().w(d).h(d).radius(d * 0.5).fill(kit::bone_fill()));
    let head = row().key(format!("bone-shelf-head-{i}")).w_full().items_center().gap(10.0).child(mark).child(kit::text_bone(
        format!("bone-shelf-name-{i}"),
        th::H1.line_height,
        14.0,
        150.0 - 30.0 * i as f32,
    ));
    let mut cards = row().key(format!("bone-shelf-row-{i}")).w_full().gap(CARD_GAP).py(6.0);
    for j in 0..n {
        cards = cards.child(
            col().key(format!("bone-card-{i}-{j}")).w(size).gap(10.0).child(el().w(size).h(size).radius(CARD_R).fill(kit::bone_fill())).child(
                col()
                    .w(size)
                    .gap(2.0)
                    .child(kit::text_bone(format!("bone-card-t-{i}-{j}"), th::BODY_M.line_height, 10.0, size * (0.78 - 0.07 * (j % 3) as f32)))
                    .child(kit::text_bone(format!("bone-card-s-{i}-{j}"), th::SMALL.line_height, 8.0, size * (0.5 - 0.06 * (j % 2) as f32)))
                    .child(kit::text_bone(format!("bone-card-f-{i}-{j}"), th::TIME.line_height, 7.0, size * 0.3).mt(2.0)),
            ),
        );
    }
    col().key(format!("bone-shelf-{i}")).w_full().gap(16.0).transition(Transition::opacity().springs(th::RISE)).child(head).child(cards)
}

pub(super) fn shelf(s: &State, p: &str, i: usize, sh: &Shelf, w: f32) -> El {
    let lang = s.lang;
    let title = match sh.kind {
        ShelfKind::Personal => S::ShelfPersonal,
        ShelfKind::Editorial => S::ShelfEditorial,
        ShelfKind::NewReleases => S::ShelfNew,
        ShelfKind::Chart => S::ShelfChart,
    };
    let (_, size) = fit(w, 144.0, GRID_GAP, 7);
    let size = size.min(200.0);
    let cards: Vec<El> = sh
        .items
        .iter()
        .map(|it| match it {
            ShelfItem::Playlist(pl) => playlist_card(s, pl, size),
            ShelfItem::Album(a) => album_card(s, a, size),
            ShelfItem::Track(t) => track_card(s, t, size, i),
        })
        .collect();
    let head = row()
        .key(format!("{p}shelf-head-{i}"))
        .w_full()
        .items_center()
        .gap(10.0)
        .child({
            let mut rng = Rng::new(s.home_seed ^ (i as u64 + 1).wrapping_mul(0x51_7cc1_b727_220a));
            let (family, hue) = match sh.kind {
                ShelfKind::Personal => (Family::Heart, 348.0),
                ShelfKind::Editorial => (Family::Star, 205.0),
                ShelfKind::NewReleases => (Family::Sun, 42.0),
                ShelfKind::Chart => (Family::Crown, 16.0),
            };
            blobs::shape(format!("{p}shelf-mark-{i}-{}", s.home_seed), family, rng.seed(), Fill::Hue(hue), Look::default(), 36.0, 36.0)
        })
        .child(label(title.get(lang).to_owned(), th::H1, th::text()).truncate().min_w(0.0));
    let row_key = format!("{p}shelf-row-{i}");
    let head = match row_arrows(s, &row_key, sh.items.len(), size, w) {
        Some(a) => head.child(el().grow(1.0)).child(a),
        None => head,
    };
    col().key(format!("{p}shelf-{i}")).gap(16.0).child(head).child(hscroll(row_key, cards))
}
