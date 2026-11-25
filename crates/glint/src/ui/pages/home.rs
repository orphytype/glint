//! Home page (full-height fluid Wave stage + infinite arc wheel) and dedicated Explore page
//! (catalog shelves: Chart, New Releases, For You, Editorial).

use rune::{El, Transition, col, el, icon, row};
use ym_api::{Shelf, ShelfItem, ShelfKind};

use super::garden::{bug_layer, garden};
use super::shared::{CARD_GAP, CARD_R, GRID_GAP, album_card_lazy, column, fit, hscroll, hscroll_range, playlist_card_lazy, row_arrows, track_card_lazy};
use crate::app::{Load, State};
use crate::i18n::S;
use crate::theme as th;
use crate::ui::icons;
use crate::ui::kit::{self, RAIL_W, label};

/// Home screen: full-height 3D-textured fluid Wave stage paired with the infinite My Wave arc wheel.
pub(super) fn home(s: &State, w: f32, h: f32) -> El {
    crate::app::PAGE_KEY.with_borrow_mut(String::clear);
    let page_w = (w - RAIL_W).max(320.0);
    let mut top = el().key("home-top").w(page_w).h(h).transition(Transition::none()).child(garden(s, page_w, h));
    if s.bugs_alive() && s.swarm.seed == s.home_seed && !s.np_open {
        top = top.child(bug_layer(s));
    }
    row()
        .key("home")
        .pin(Some(RAIL_W), Some(0.0), Some(0.0), Some(0.0))
        .transition(Transition::none())
        .child(top)
}

/// Dedicated Explore page holding all catalog shelves (Chart, New Releases, For You, Editorial).
pub(super) fn explore(s: &State, w: f32) -> El {
    let lang = s.lang;
    let cw = column(w);
    let compact = cw < 840.0;
    let mut body = col().gap(if compact { 28.0 } else { 34.0 }).child(kit::rise(kit::page_title(S::Explore.get(lang), "").mt(8.0), 0));
    match &s.home {
        Load::Ready(shelves) => {
            let mut order: Vec<usize> = (0..shelves.len()).collect();
            order.sort_by_key(|&i| (std::cmp::Reverse(shelves[i].items.len()), shelves[i].kind != ShelfKind::NewReleases));
            for (pos, (i, sh)) in order.into_iter().map(|i| (i, &shelves[i])).enumerate() {
                if sh.items.is_empty() {
                    continue;
                }
                body = body.child(kit::rise(shelf(s, "ex-", i, sh, w), 1 + pos.min(3)));
            }
        }
        Load::Failed(_) => {
            if let Some(e) = kit::list_state(&s.home, s) {
                body = body.child(e);
            }
        }
        _ => {
            for i in 0..3 {
                body = body.child(shelf_bones(w, i));
            }
        }
    }
    kit::scroll_page_live("explore", body)
}

fn shelf_bones(w: f32, i: usize) -> El {
    let (_, size) = fit(w, 144.0, GRID_GAP, 7);
    let size = size.min(200.0);
    let n = (((column(w) + CARD_GAP) / (size + CARD_GAP)).floor() as usize).max(1);
    let mark = el().w(24.0).h(24.0).radius(7.0).fill(kit::bone_fill());
    let head = row().key(format!("bone-shelf-head-{i}")).w_full().items_center().gap(9.0).child(mark).child(kit::text_bone(
        format!("bone-shelf-name-{i}"),
        th::SECTION.line_height,
        15.0,
        150.0 - 24.0 * i as f32,
    ));
    let mut cards = row().key(format!("bone-shelf-row-{i}")).w_full().gap(CARD_GAP).py(4.0);
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
    col().key(format!("bone-shelf-{i}")).w_full().gap(14.0).transition(Transition::opacity().springs(th::RISE)).child(head).child(cards)
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
    let row_key = format!("{p}shelf-row-{i}");
    let vis = hscroll_range(&row_key, sh.items.len(), size, w);
    let cards: Vec<El> = sh
        .items
        .iter()
        .enumerate()
        .map(|(idx, it)| {
            let load = vis.contains(&idx);
            match it {
                ShelfItem::Playlist(pl) => playlist_card_lazy(s, pl, size, load),
                ShelfItem::Album(a) => album_card_lazy(s, a, size, load),
                ShelfItem::Track(t) => track_card_lazy(s, t, size, i, load),
            }
        })
        .collect();
    let light = th::light();
    let (ic, tint) = match sh.kind {
        ShelfKind::Personal => (&icons::SHELF_PERSONAL, if light { th::rgb(0xd8345f) } else { th::rgb(0xff5b7f) }),
        ShelfKind::Editorial => (&icons::SHELF_EDITORIAL, if light { th::rgb(0x2878d8) } else { th::rgb(0x52a8ff) }),
        ShelfKind::NewReleases => (&icons::SHELF_NEW, if light { th::rgb(0xc68200) } else { th::rgb(0xffd24c) }),
        ShelfKind::Chart => (&icons::SHELF_CHART, if light { th::rgb(0xcf5616) } else { th::rgb(0xff8a3d) }),
    };
    let mark = row()
        .key(format!("{p}shelf-mark-{i}"))
        .w(24.0)
        .h(24.0)
        .radius(7.0)
        .center()
        .fill(tint.alpha(if light { 0.14 } else { 0.16 }))
        .child(icon(ic, 14.0).color(tint));
    let head = row()
        .key(format!("{p}shelf-head-{i}"))
        .w_full()
        .h(32.0)
        .items_center()
        .gap(9.0)
        .child(mark)
        .child(label(title.get(lang).to_owned(), th::SECTION, th::text()).truncate().min_w(0.0));
    let head = match row_arrows(s, &row_key, sh.items.len(), size, w) {
        Some(a) => head.child(el().grow(1.0)).child(a),
        None => head,
    };
    col().key(format!("{p}shelf-{i}")).gap(12.0).child(head).child(hscroll(s, row_key, size, cards))
}
