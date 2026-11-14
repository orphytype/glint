//! The first screen: sign in with your own Yandex account (the app only ever asks you to open
//! Yandex's page and paste back what it gives you), or try the offline demo.
//!
//! On the left, over the welcome, a loose cluster of the app's own shapes (a heart, a star, a sun,
//! a drop, a moon, a bolt), each dithered in its colour and different every launch; the form sits
//! on a pane of glass on the right.

use rune::{Align, Cursor, El, Transition, Tx, col, el, icon, row, text_input};

use super::icons;
use super::blobs::{self, Family, Fill, Look, Rng};
use super::kit::{self, label};
use super::shapes::{self, Ill};
use crate::app::State;
use crate::fx::Glass;
use crate::i18n::S;
use crate::theme::{self as th, veil};

/// Shown while a remembered token is being checked, so the sign-in form never flashes up.
fn splash(s: &State) -> El {
    let p = s.pal();
    row().key("login").absolute(0.0, 0.0).w_full().h_full().center().child(
        col().gap(26.0).items_center().child(shapes::ill(Ill::Record, 200.0, p.accent, 0.0)).child(kit::caps(S::SigningIn.get(s.lang), th::text2())),
    )
}

pub fn page(s: &State) -> El {
    if s.login.busy && s.login.token.text.is_empty() {
        return splash(s);
    }
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
    let input = row().key("token-field").grow(1.0).min_w(0.0).h(kit::BTN_H).pl(14.0).pr(0.0).gap(4.0).items_center().radius(kit::FIELD_R).clip_rounded(kit::FIELD_R).fill(veil(0.05)).border(1.0, veil(0.11)).child(field).child(
        kit::ibtn("reveal", &icons::LOCK, kit::GLYPH, 40.0, if s.login.reveal { th::text() } else { th::text3() }, S::TokenHint.get(lang))
            .on_press(|s: &mut State| s.login.reveal = !s.login.reveal),
    );

    let step = |n: &str, text: &str| row().w_full().gap(12.0).items_center().child(row().w(24.0).h(24.0).radius(12.0).center().fill(veil(0.08)).child(label(n.to_owned(), th::LABEL, th::text2()))).child(label(text.to_owned(), th::SMALL, th::text2()).wrap().grow(1.0).min_w(0.0));

    let mut card = col().key("login-card").w(420.0).padding(32.0).gap(16.0).transition(Transition::none()).child(kit::glass(Glass::Card, 28.0));
    if !wide {
        card = card.child(label(S::Welcome.get(lang), th::H1, th::text()).wrap().w_full()).child(label(S::LoginLead.get(lang), th::SMALL, th::text2()).wrap().w_full());
    }
    card = card
        .child(step("1", S::OpenYandex.get(lang)))
        .child(kit::btn_primary("open-yandex", S::OpenYandex.get(lang), Some(&icons::EXTERNAL)).w_full().on_press_tx(|_s: &mut State, tx: &mut Tx| {
            let _ = tx.open_url(&ym_api::auth::authorize_url());
        }))
        .child(step("2", S::LoginStep2.get(lang)).mt(6.0))
        .child(row().w_full().gap(8.0).items_center().child(input).child(
            kit::btn_primary("sign-in", if busy { S::SigningIn.get(lang) } else { S::SignIn.get(lang) }, None).px(20.0).opacity(if busy { 0.6 } else { 1.0 }).on_press(|s: &mut State| {
                if !s.login.busy {
                    s.submit_token();
                }
            }),
        ));
    if let Some(e) = s.login.error {
        card = card.child(label(e.get(lang), th::SMALL, th::rgb(0xe5503f)).wrap().w_full());
    }
    card = card
        .child(el().w_full().h(1.0).my(8.0).fill(th::hairline()))
        .child(kit::btn_soft("demo", S::TryDemo.get(lang), Some(&icons::SPARKLE)).w_full().on_press(|s: &mut State| s.start_demo()))
        .child(label(S::DemoNote.get(lang), th::SMALL, th::text3()).align(Align::Center).wrap().w_full())
        .child(
            row()
                .w_full()
                .gap(10.0)
                .items_start()
                .mt(4.0)
                .child(icon(&icons::LOCK, 16.0).color(th::text3()))
                .child(label(S::PrivacyNote.get(lang), th::SMALL, th::text3()).wrap().grow(1.0)),
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
        (Family::Bolt, 14.0, 74.0, 122.0, 118.0),
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
