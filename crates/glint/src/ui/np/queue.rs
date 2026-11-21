//! Virtualized Now Playing queue panel with history, active source heading, and up-next section.

use std::borrow::Cow;

use rune::{El, GrowthSpec, ScrollSpec, col, el, row};

use super::notice;
use crate::app::State;
use crate::i18n::S;
use crate::theme::{self as th, Pal};
use crate::ui::icons;
use crate::ui::kit::{self, Hover as _, ROW_H, TITLE_H, label};
use crate::ui::shapes::Ill;

/// Top offset of the queue list below the panel edge.
pub const QUEUE_TOP: f32 = TITLE_H + 16.0;

/// Scroll offset showing the track before `cur` at the top of the panel.
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
            QueueItem::Track(q) => Cow::Owned(format!("q{q}-{}", s.player.queue.get(q).map_or("", |t| t.id.as_str()))),
            QueueItem::Now => Cow::Borrowed("q-now"),
            QueueItem::Next => Cow::Borrowed("q-next"),
        },
        move |s: &State, i: usize| {
            kit::built_under(under, || match queue_item(cur, i) {
                QueueItem::Track(q) => match s.player.queue.get(q) {
                    Some(t) if q < cur => el().key(format!("qh-{q}")).w_full().hov_group().child(kit::queue_row(s, t, q).gop(0.5, 1.0)),
                    Some(t) => kit::queue_row(s, t, q),
                    None => el().h(ROW_H),
                },
                QueueItem::Now => now_heading(s),
                QueueItem::Next => {
                    let after = s.player.queue.len().saturating_sub(cur + 1);
                    let clear = kit::ibtn("q-clear", &icons::TRASH, 15.0, 28.0, th::text3(), S::ClearQueue.get(s.lang)).on_press(State::clear_up_next);
                    row()
                        .key("q-next-h")
                        .w_full()
                        .h(ROW_H)
                        .px(kit::ROW_PAD)
                        .pt(18.0)
                        .gap(6.0)
                        .items_center()
                        .child(label(S::UpNext.get(s.lang), th::H3, th::text()))
                        .child(el().grow(1.0))
                        .child(label(s.lang.tracks(after), th::SMALL, th::text3()))
                        .child(clear)
                }
            })
        },
    );
    col()
        .key("np-queue")
        .absolute(x, 0.0)
        .w(w)
        .h(h)
        .motion(rune::Spring::NONE)
        .scroll(ScrollSpec { stick_bottom: false })
        .scroll_edge_fade(44.0)
        .child(col().w_full().pt(QUEUE_TOP).pb((h * 0.5).max(72.0)).child(rows))
}

fn now_heading(s: &State) -> El {
    row().key("q-now-h").w_full().h(ROW_H).px(kit::ROW_PAD).pt(18.0).items_center().child(label(S::NowPlaying.get(s.lang), th::H3, th::text()))
}
