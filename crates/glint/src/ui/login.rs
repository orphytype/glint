//! The first screen: sign in with your own Yandex account (the app only ever asks you to open
//! Yandex's page and paste back what it gives you).
//!
//! On the left, over the welcome, a loose cluster of the app's own shapes (a heart, a star, a sun,
//! a drop, a moon, a bolt), each dithered in its colour and different every launch; the form sits
//! on a pane of glass on the right.

use rune::{Cursor, El, Transition, Tx, col, el, icon, row, text_input};

use super::blobs::{self, Family, Fill, Look, Rng};
use super::icons;
use super::kit::{self, label};
use crate::app::State;
use crate::fx::Glass;
use crate::i18n::S;
use crate::theme::{self as th, veil};

pub fn page(s: &State) -> El {
    let p = s.pal();
    let lang = s.lang;
    let busy = s.login.busy;
    let w = s.win_w.get().max(960.0);
    let wide = w >= 1080.0;

    let mut field = text_input(&s.login.token, th::BODY)
        .key("token")
        .single_line()
        .grow(1.0)
        .min_w(0.0)
        .color(th::text())
        .placeholder(S::TokenHint.get(lang), th::text3())
        .caret(p.accent, p.accent.alpha(0.30))
        .on_edit(|s: &mut State| &mut s.login.token)
        .on_submit(|s: &mut State, _tx: &mut Tx| s.submit_token());
    if !s.login.reveal {
        field = field.secure();
    }
    let input = row()
        .key("token-field")
        .grow(1.0)
        .min_w(0.0)
        .h(kit::BTN_H)
        .pl(14.0)
        .pr(2.0)
        .items_center()
        .radius(kit::FIELD_R)
        .clip_rounded(kit::FIELD_R)
        .fill(veil(0.05))
        .border(1.0, veil(0.11))
        .child(field.mr(4.0))
        .child(kit::ibtn("paste-token", &icons::PASTE, 16.0, 32.0, th::text3(), S::Paste.get(lang)).on_press_tx(|_s: &mut State, tx: &mut Tx| {
            tx.read_clipboard(|s: &mut State, content, _| {
                let pasted = content.text().trim();
                if !pasted.is_empty() {
                    s.login.token = rune::TextEdit::new(pasted);
                    s.login.token.set_secure(!s.login.reveal);
                    s.login.error = None;
                }
            });
        }))
        .child(
            // Show or hide what was pasted. `secure` marks the edit itself, so showing has to
            // clear that mark on the edit, not only stop asking for it.
            kit::ibtn(
                "reveal",
                if s.login.reveal { &icons::EYE_OFF } else { &icons::EYE },
                16.0,
                32.0,
                if s.login.reveal { th::text() } else { th::text3() },
                if s.login.reveal { S::HideToken.get(lang) } else { S::ShowToken.get(lang) },
            )
            .on_press(|s: &mut State| {
                s.login.reveal = !s.login.reveal;
                s.login.token.set_secure(!s.login.reveal);
            }),
        );

    // A numbered step: what to do, then the controls for it.
    let step = |n: &str, text: &str, controls: El| {
        let head = row()
            .w_full()
            .gap(10.0)
            .items_center()
            .child(row().w(22.0).h(22.0).radius(11.0).center().fill(veil(0.08)).child(label(n.to_owned(), th::LABEL, th::text2())))
            .child(label(text.to_owned(), th::SMALL, th::text2()).wrap().grow(1.0).min_w(0.0));
        col().w_full().gap(12.0).child(head).child(controls)
    };

    let open = row()
        .w_full()
        .gap(8.0)
        .items_center()
        .child(kit::btn_primary("open-yandex", S::OpenYandex.get(lang), Some(&icons::EXTERNAL)).grow(1.0).min_w(0.0).on_press_tx(|_s: &mut State, tx: &mut Tx| {
            let _ = tx.open_url(&ym_api::auth::authorize_url());
        }))
        .child(kit::btn_soft_icon("copy-yandex", &icons::COPY, S::CopyLoginLink.get(lang)).on_press_tx(|s: &mut State, tx: &mut Tx| {
            tx.copy(ym_api::auth::authorize_url());
            let text = s.tr(S::LinkCopied);
            s.toast(text);
        }));
    let sign_in = row().w_full().gap(8.0).items_center().child(input).child(
        kit::btn_primary("sign-in", if busy { S::SigningIn.get(lang) } else { S::SignIn.get(lang) }, None).px(20.0).opacity(if busy { 0.6 } else { 1.0 }).on_press(|s: &mut State| {
            if !s.login.busy {
                s.submit_token();
            }
        }),
    );

    let mut card = col().key("login-card").w(420.0).padding(28.0).transition(Transition::none()).child(kit::glass(Glass::Card, 28.0));
    if !wide {
        card = card
            .child(label(S::Welcome.get(lang), th::H1, th::text()).wrap().w_full())
            .child(label(S::LoginLead.get(lang), th::SMALL, th::text2()).wrap().w_full().mt(8.0).mb(24.0));
    }
    card = card.child(step("1", S::LoginStep1.get(lang), open)).child(step("2", S::LoginStep2.get(lang), sign_in).mt(24.0));
    if let Some(e) = s.login.error {
        card = card.child(label(e.get(lang), th::SMALL, th::rgb(0xe5503f)).wrap().w_full().mt(10.0));
    }
    card = card.child(el().w_full().h(1.0).mt(24.0).mb(18.0).fill(th::hairline())).child(
        row()
            .w_full()
            .gap(10.0)
            .items_start()
            .child(icon(&icons::LOCK, 15.0).color(th::text3()).mt(1.0))
            .child(label(if cfg!(windows) { S::PrivacyNote } else { S::PrivacyNoteFile }.get(lang), th::SMALL, th::text3()).wrap().grow(1.0).min_w(0.0)),
    );

    let welcome = col()
        .w(440.0)
        .gap(18.0)
        .child(cluster(s))
        .child(kit::rise(label(S::Welcome.get(lang), th::TITLE, th::text()).wrap().w_full(), 1))
        .child(kit::rise(label(S::LoginLead.get(lang), th::LEAD, th::text2()).wrap().w_full(), 2));
    let mut r = row().key("login-row").absolute(0.0, 0.0).w_full().h_full().center().gap(96.0).cursor(Cursor::Default).transition(Transition::none());
    if wide {
        r = r.child(welcome);
    }
    el().key("login").absolute(0.0, 0.0).w_full().h_full().transition(Transition::none()).child(r.child(card))
}

/// The shapes over the welcome: each in its place, but its own size, lean and outline every launch.
fn cluster(s: &State) -> El {
    let mut rng = Rng::new(s.home_seed ^ 0x6c6f_6769);
    // family, hue, side, x, y (of a 440 by 196 box)
    let parts: [(Family, f32, f32, f32, f32); 6] = [
        (Family::Heart, 348.0, 132.0, 0.0, 52.0),
        (Family::Star, 205.0, 98.0, 116.0, 0.0),
        (Family::Sun, 42.0, 112.0, 196.0, 70.0),
        (Family::Drop, 228.0, 86.0, 300.0, 0.0),
        (Family::Moon, 172.0, 104.0, 332.0, 86.0),
        (Family::Crown, 14.0, 74.0, 122.0, 118.0),
    ];
    let mut b = el().key("login-shapes").w(440.0).h(196.0).mb(8.0).transition(Transition::none());
    for (i, (family, hue, side, x, y)) in parts.into_iter().enumerate() {
        let side = side * rng.range(0.92, 1.06);
        let (jx, jy) = (rng.range(-6.0, 6.0), rng.range(-6.0, 6.0));
        let look = Look { order: 1 + i as u32, ..Look::default() };
        b = b.child(blobs::shape(format!("login-shape-{i}"), family, rng.seed(), Fill::Hue(hue + rng.range(-12.0, 12.0)), look, side, side).absolute(x + jx, y + jy));
    }
    b
}
