//! Virtualized Now Playing queue panel with history, active source heading, and up-next section.

use std::borrow::Cow;

use rune::{Cursor, El, GrowthSpec, ScrollSpec, col, el, row};

use super::notice;
use crate::app::State;
use crate::i18n::S;
use crate::theme::{self as th, Pal};
use crate::ui::kit::{self, Hover as _, ROW_H, TITLE_H, label};
use crate::ui::shapes::Ill;

/// Top breathing room inside the queue viewport (starts right below the title bar).
pub const QUEUE_TOP: f32 = 24.0;

/// Scroll offset showing the track before `cur` peeking gently above Now Playing.
pub fn queue_offset(cur: usize) -> f32 {
    cur.saturating_sub(1) as f32 * ROW_H
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum QueueItem {
    Track(usize),
    Now,
    Next,
}

fn queue_item(cur: usize, i: usize) -> QueueItem {
    if i < cur {
        QueueItem::Track(i)
    } else if i == cur {
        QueueItem::Now
    } else if i == cur + 1 {
        QueueItem::Track(cur)
    } else if i == cur + 2 {
        QueueItem::Next
    } else {
        QueueItem::Track(i - 2)
    }
}

pub(super) fn queue_view(s: &State, p: &Pal, x: f32, w: f32, h: f32) -> El {
    let lang = s.lang;
    let n = s.player.queue.len();
    if n == 0 {
        return notice(p, x, w, h, Ill::Tape, S::QueueEmpty.get(lang));
    }
    let cur = s.player.pending_index.or(s.player.index).unwrap_or(0).min(n - 1);
    let after = n - cur - 1;
    let total = cur + 2 + if after > 0 { 1 + after } else { 0 };
    let under = kit::under();
    let rows = col().key("np-queue-rows").w_full().growth(GrowthSpec::default()).virtual_rows(
        total,
        ROW_H,
        move |s: &State, i: usize| match queue_item(cur, i) {
            QueueItem::Track(q) => {
                let id = s.player.queue.get(q).map_or("", |t| t.id.as_str());
                let occ = s.player.queue.iter().take(q).filter(|t| t.id == id).count();
                Cow::Owned(format!("q-{id}-{occ}"))
            }
            QueueItem::Now => Cow::Borrowed("q-now"),
            QueueItem::Next => Cow::Borrowed("q-next"),
        },
        move |s: &State, i: usize| {
            kit::built_under(under, || match queue_item(cur, i) {
                QueueItem::Track(q) => match s.player.queue.get(q) {
                    Some(t) => kit::queue_row(s, t, q, q < cur),
                    None => el().h(ROW_H),
                },
                QueueItem::Now => now_heading(s),
                QueueItem::Next => next_heading(s, s.player.queue.len().saturating_sub(cur + 1)),
            })
        },
    );
    let vp_h = (h - TITLE_H).max(120.0);
    col()
        .key("np-queue")
        .absolute(x, TITLE_H)
        .w(w)
        .h(vp_h)
        .motion(rune::Spring::NONE)
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(44.0)
        .child(col().w_full().pt(QUEUE_TOP).pb((h * 0.48).max(72.0)).child(rows))
}

fn now_heading(s: &State) -> El {
    col().key("q-now-h").w_full().h(ROW_H).px(kit::ROW_PAD).justify_end().pb(6.0).child(row().w_full().h(26.0).items_center().child(label(
        S::NowPlaying.get(s.lang),
        th::H3,
        th::text2(),
    )))
}

fn next_heading(s: &State, after: usize) -> El {
    let lang = s.lang;
    let count_text = lang.tracks(after);
    let clear_text = S::ClearQueue.get(lang);
    let count_w = rune::text::measure(&count_text, &[], th::SMALL, None).0.ceil();
    let clear_w = rune::text::measure(clear_text, &[], th::SMALL, None).0.ceil();
    let morph_w = count_w.max(clear_w) + 2.0;
    let morph_clear = row()
        .key("q-clear")
        .role("button")
        .label(clear_text)
        .h(26.0)
        .px(8.0)
        .mr(-8.0)
        .radius(8.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .hov(th::veil(0.08))
        .press(th::veil(0.14))
        .press_scale(0.97)
        .hov_group()
        .on_press(State::clear_up_next)
        .child(
            el().w(morph_w)
                .h(20.0)
                .child(row().absolute(0.0, 0.0).w_full().h_full().items_center().justify_end().child(label(count_text, th::SMALL, th::text3()).gop(1.0, 0.0)))
                .child(row().absolute(0.0, 0.0).w_full().h_full().items_center().justify_end().child(label(clear_text, th::SMALL, th::text()).gop(0.0, 1.0))),
        );
    col()
        .key("q-next-h")
        .w_full()
        .h(ROW_H)
        .px(kit::ROW_PAD)
        .justify_end()
        .pb(6.0)
        .child(row().w_full().h(26.0).gap(8.0).items_center().child(label(S::UpNext.get(lang), th::H3, th::text2())).child(el().grow(1.0)).child(morph_clear))
}
