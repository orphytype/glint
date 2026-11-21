//! Menus, popovers, volume card, details sheet, and Now Playing panel visibility.

use rune::PopupAnchor;
use ym_api::Track;

use super::{Load, NpTab, Pop, Popover, Sheet, State, TrackMenu};

impl State {
    /// Closes the dock volume popover and resets its hover grace state.
    pub fn close_volume(&mut self) {
        self.vol_open = false;
        self.vol_button_hover = false;
        self.vol_card_hover = false;
        self.vol_close_at = None;
    }

    /// Whether the profile menu is visible (open or still closing).
    pub fn menu_visible(&self) -> bool {
        self.menu_open || self.menu.t > 0.001 || self.menu.v != 0.0
    }

    /// Steps every menu, popover, sheet, and Now Playing spring.
    pub(crate) fn tick_menu(&mut self, dt: f32) -> bool {
        let reduce = self.settings.reduce_motion;
        let np_target = self.np_open && self.current().is_some();
        let veil_target = np_target && self.np.t >= 1.0;
        let mut changed = self.menu.step(self.menu_open, dt, reduce)
            | self.tmenu.step(self.tmenu_open, dt, reduce)
            | self.vol.step(self.vol_open, dt, reduce)
            | self.np.glide(np_target, dt, reduce)
            | self.np_veil.step(veil_target, dt, reduce)
            | self.pop.step(self.popover_open, dt, reduce)
            | self.sheet_pop.morph(self.sheet_open, dt, reduce)
            | (!self.sheet_dragging && self.sheet_drag.settle(0.0, 0.42, 0.86, dt));
        if !self.sheet_open && self.sheet_pop.t == 0.0 && self.sheet_pop.v == 0.0 && self.sheet.is_some() {
            self.sheet = None;
            changed = true;
        }
        if !self.popover_open && self.pop.t == 0.0 && self.pop.v == 0.0 && self.popover.is_some() {
            self.popover = None;
            changed = true;
        }
        if !self.tmenu_open && self.tmenu.t == 0.0 && self.tmenu.v == 0.0 && self.track_menu.is_some() {
            self.track_menu = None;
            changed = true;
        }
        changed
    }

    /// Whether any menu, sheet, or overlay spring is still animating.
    pub fn menu_moving(&self) -> bool {
        self.menu.moving(self.menu_open)
            || self.tmenu.moving(self.tmenu_open)
            || self.vol.moving(self.vol_open)
            || self.np.moving(self.np_open && self.current().is_some())
            || self.np_veil.moving(self.np_open && self.current().is_some() && self.np.t >= 1.0)
            || self.pop.moving(self.popover_open)
            || self.sheet_pop.moving(self.sheet_open)
            || (!self.sheet_dragging && (self.sheet_drag.t != 0.0 || self.sheet_drag.v != 0.0))
    }

    /// Whether the volume card is visible (open or still closing).
    pub fn vol_visible(&self) -> bool {
        self.vol_open || self.vol.t > 0.001 || self.vol.v != 0.0
    }

    /// Opens the context menu for `track` anchored to a cursor point or element key.
    pub fn open_track_menu(&mut self, track: Track, queue_index: Option<usize>, anchor: PopupAnchor) {
        self.menu_open = false;
        self.close_volume();
        self.track_menu = Some(TrackMenu { track, queue_index, anchor, player: false, choosing: false });
        self.tmenu_open = true;
    }

    /// Opens the context menu for the playing track from the bar or Now Playing screen.
    pub fn open_player_menu(&mut self, anchor: PopupAnchor) {
        let Some(t) = self.current().cloned() else { return };
        self.open_track_menu(t, None, anchor);
        if let Some(m) = &mut self.track_menu {
            m.player = true;
        }
    }

    /// Opens the details modal for a track or the current page.
    pub fn open_sheet(&mut self, what: Sheet) {
        self.close_track_menu();
        self.close_popover();
        self.menu_open = false;
        self.close_volume();
        if let Sheet::Track(t) = &what {
            self.load_sheet_credits(t);
        }
        self.sheet = Some(what);
        self.sheet_open = true;
        self.sheet_seq += 1;
        self.sheet_drag = Pop::default();
        self.sheet_dragging = false;
    }

    /// Fetches track credits (performers, authors, label) once per track id.
    fn load_sheet_credits(&mut self, track: &Track) {
        if self.sheet_credits.0.as_deref() == Some(track.id.as_str()) && matches!(self.sheet_credits.1, Load::Ready(_) | Load::Loading) {
            return;
        }
        self.sheet_credits = (Some(track.id.clone()), Load::Loading);
        let (b, t, id) = (self.backend.clone(), track.clone(), track.id.clone());
        self.job(async move { b.credits(&t).await }, move |s, _, res| {
            if s.sheet_credits.0.as_deref() != Some(id.as_str()) {
                return;
            }
            s.sheet_credits.1 = match res {
                Ok(c) => Load::Ready(c),
                Err(e) => Load::Failed(e.to_string()),
            };
        });
    }

    pub fn sheet_visible(&self) -> bool {
        self.sheet.is_some() && (self.sheet_open || self.sheet_pop.t > 0.001 || self.sheet_pop.v != 0.0)
    }

    pub fn close_track_menu(&mut self) {
        self.tmenu_open = false;
    }

    /// Opens a popover anchored to `p.anchor()`, or toggles it closed if the same anchor is active.
    pub fn open_popover(&mut self, p: Popover) {
        let again = self.popover_open && self.popover.as_ref().is_some_and(|q| q.anchor() == p.anchor());
        self.menu_open = false;
        self.tmenu_open = false;
        self.close_volume();
        if again {
            self.popover_open = false;
            return;
        }
        if !self.popover_open {
            self.pop = Pop::default();
        }
        self.popover = Some(p);
        self.popover_open = true;
    }

    pub fn close_popover(&mut self) {
        self.popover_open = false;
    }

    pub fn popover_visible(&self) -> bool {
        self.popover.is_some() && (self.popover_open || self.pop.t > 0.001 || self.pop.v != 0.0)
    }

    /// Whether lyrics for the current track were checked and none are available.
    pub fn lyrics_missing(&self) -> bool {
        if self.current().is_none() {
            return false;
        }
        match &self.lyrics.lyrics {
            Load::Failed(_) => true,
            Load::Ready(l) => l.instrumental || !l.has_words(),
            _ => false,
        }
    }

    /// Active side panel on Now Playing, keeping the lyrics panel steady while the next track's
    /// lyrics are in flight so it never flashes shut between tracks.
    pub fn np_panel(&self) -> NpTab {
        if self.np_tab != NpTab::Lyrics {
            return self.np_tab;
        }
        let there = match &self.lyrics.lyrics {
            Load::Ready(l) => {
                let v = !l.instrumental && l.has_words();
                self.ly_panel.set(v);
                v
            }
            Load::Failed(_) => {
                self.ly_panel.set(false);
                false
            }
            _ => self.ly_panel.get(),
        };
        if there { NpTab::Lyrics } else { NpTab::Closed }
    }

    pub fn track_menu_visible(&self) -> bool {
        self.track_menu.is_some() && (self.tmenu_open || self.tmenu.t > 0.001 || self.tmenu.v != 0.0)
    }
}
