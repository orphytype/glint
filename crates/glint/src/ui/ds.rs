//! RUNE's own component kit (`rune-kit`: switch, segmented control, select, button), dressed in
//! Glint's colours. Settings is built from it. The look is the quiet one of the rest of the app:
//! the ink of the page, hairline edges, small radii, no shadows.

use rune::{Color, TextStyle};
use rune_kit::{Elevation, Kit, Radius};

use crate::app::State;
use crate::theme as th;

/// The kit for the current look (light or dark) and the accent in use.
pub fn kit(s: &State) -> Kit {
    let mut k = Kit::neutral(!th::light());
    let accent = s.pal().accent;
    let (text, text2, text3) = (th::text(), th::text2(), th::text3());
    let edge = th::veil(0.14);
    let faint = th::veil(0.08);
    let track = th::veil(0.18);
    let card = th::pop();
    let on_solid = th::on_solid();
    let white = Color::WHITE;

    let c = &mut k.c;
    c.page = th::bg();
    c.bg_base = card;
    c.bg_deep = th::bg();
    c.layer_01 = faint;
    c.layer_02 = faint;
    c.layer_03 = track;
    c.layer_04 = track;
    c.bg_accent = accent;
    c.bg_inverse = th::solid();
    c.bg_contrast = th::solid();
    c.bg_button_neutral = faint;
    c.text_base = text;
    c.text_strong = text;
    c.text_muted = text2;
    c.text_weak = text2;
    c.text_faint = text3;
    c.text_inverse = on_solid;
    c.text_contrast = on_solid;
    c.text_accent = accent;
    c.icon_base = text;
    c.icon_muted = text2;
    c.icon_faint = text3;
    c.icon_weaker = text3;
    c.icon_inverse = on_solid;
    c.icon_contrast = on_solid;
    c.icon_accent = accent;
    c.border_base = edge;
    c.border_muted = faint;
    c.border_strong = edge;
    c.border_weak = faint;
    c.border_hover = edge;
    c.border_focus = th::veil(0.5);
    c.std_border = edge;
    c.std_text = text;
    c.hover = th::veil(0.06);
    c.pressed = th::veil(0.10);
    c.surface_base = faint;
    c.surface_weak = faint;
    c.surface_hover = th::veil(0.06);
    c.surface_raised = card;
    c.surface_raised_base = card;
    c.surface_raised_hover = th::veil(0.06);
    c.surface_float = card;
    c.knob = white;
    c.knob_on = white;

    k.elevation = Elevation::default();
    k.radius = Radius { xs: 2.0, sm: 3.0, md: 5.0, lg: 6.0, xl: 8.0 };

    // Bahnschrift, as everywhere else.
    let face = |t: TextStyle| t.family(th::SANS);
    let ty = &mut k.ty;
    ty.ui = face(ty.ui);
    ty.ui_medium = face(ty.ui_medium);
    ty.ui_compact = face(ty.ui_compact);
    ty.small = face(ty.small);
    ty.small_medium = face(ty.small_medium);
    ty.body = face(ty.body);
    ty.body_medium = face(ty.body_medium);
    ty.text_small = face(ty.text_small);
    ty.text_small_medium = face(ty.text_small_medium);
    ty.text_xsmall = face(ty.text_xsmall);
    ty.text_xsmall_medium = face(ty.text_xsmall_medium);
    k
}
