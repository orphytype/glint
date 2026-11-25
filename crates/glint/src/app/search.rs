//! Search queries, history, and track likes/dislikes.

use rune::TextEdit;
use ym_api::{SearchResult, Track};

use super::{Load, SEARCH_HISTORY, SearchTab, Session, State};
use crate::i18n::S;

impl State {
    pub fn run_search(&mut self, text: String) {
        let text = text.trim().to_owned();
        self.search.generation += 1;
        let generation = self.search.generation;
        self.search.relaid = None;
        if text.is_empty() {
            self.search.results = Load::Idle;
            self.search.query.clear();
            return;
        }
        self.search.query = text.clone();
        self.search.results = Load::Loading;
        self.fetch_search_all(text, generation, true);
        self.fetch_search_kind();
    }

    /// Searches everything for `text`. When nothing turns up and `retry` is set, it searches
    /// again as if the text had been typed on the other keyboard layout ("ytjy" for "неон").
    fn fetch_search_all(&mut self, text: String, generation: u64, retry: bool) {
        let (b, fc, fc_on) = (self.backend.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        let asked = text.clone();
        self.job(async move { b.search(&text, ym_api::SearchKind::All, &fc, fc_on).await }, move |s, _, res| {
            if s.search.generation != generation {
                return;
            }
            if retry
                && res.as_ref().is_ok_and(SearchResult::is_empty)
                && let Some(other) = crate::find::other_layout(&asked)
            {
                s.fetch_search_all(other, generation, false);
                return;
            }
            if !retry && res.as_ref().is_ok_and(|r| !r.is_empty()) {
                s.search.relaid = Some(asked);
                s.search.more_for = None;
                s.fetch_search_kind();
            }
            s.search.results = match res {
                Ok(r) => Load::Ready(r),
                Err(e) => Load::Failed(e.to_string()),
            };
        });
    }

    /// Picks a result filter chip; non-"All" kinds fetch a wider dedicated result set.
    pub fn set_search_tab(&mut self, tab: SearchTab) {
        if self.search.tab != tab {
            self.art.bump_page();
            self.art_trim_at = Some(self.now + 1.2);
        }
        self.search.tab = tab;
        self.fetch_search_kind();
    }

    fn fetch_search_kind(&mut self) {
        let tab = self.search.tab;
        let kind = match tab {
            SearchTab::All => return,
            SearchTab::Tracks => ym_api::SearchKind::Track,
            SearchTab::Albums => ym_api::SearchKind::Album,
            SearchTab::Artists => ym_api::SearchKind::Artist,
            SearchTab::Playlists => ym_api::SearchKind::Playlist,
        };
        if self.search.query.is_empty() || self.search.more_for.as_ref().is_some_and(|(q, t)| *t == tab && q == &self.search.query) {
            return;
        }
        let text = self.search.relaid.clone().unwrap_or_else(|| self.search.query.clone());
        self.search.more_for = Some((self.search.query.clone(), tab));
        self.search.more = Load::Loading;
        let (b, fc, fc_on) = (self.backend.clone(), self.fkcensor.clone(), self.settings.fkcensor);
        let want = (self.search.query.clone(), tab);
        self.job(async move { b.search(&text, kind, &fc, fc_on).await }, move |s, _, res| {
            if s.search.more_for.as_ref() != Some(&want) {
                return;
            }
            s.search.more = res.map_or_else(|e| Load::Failed(e.to_string()), Load::Ready);
        });
    }

    /// Results shown on the search page: the dedicated kind search once loaded, otherwise "All".
    pub fn search_view(&self) -> Option<&SearchResult> {
        let own = self.search.tab != SearchTab::All && self.search.more_for.as_ref().is_some_and(|(q, t)| *q == self.search.query && *t == self.search.tab);
        match (&self.search.more, own) {
            (Load::Ready(r), true) => Some(r),
            _ => self.search.results.get(),
        }
    }

    /// Moves the active query to the top of search history.
    pub fn remember_search(&mut self) {
        let q = self.search.query.trim().to_owned();
        if q.is_empty() {
            return;
        }
        let q_low = q.to_lowercase();
        let h = &mut self.settings.search_history;
        h.retain(|x| !x.eq_ignore_ascii_case(&q) && x.to_lowercase() != q_low);
        h.insert(0, q);
        h.truncate(SEARCH_HISTORY);
        self.mark_settings_dirty();
    }

    pub fn forget_search(&mut self, q: &str) {
        self.settings.search_history.retain(|x| x != q);
        self.mark_settings_dirty();
    }

    pub fn clear_search_history(&mut self) {
        self.settings.search_history.clear();
        self.mark_settings_dirty();
    }

    pub fn clear_listening_history(&mut self) {
        self.listening_history.clear();
    }

    /// Re-runs a query from search history.
    pub fn search_again(&mut self, q: &str) {
        self.search.edit = TextEdit::new(q);
        self.search.seen = q.to_owned();
        self.run_search(q.to_owned());
        self.remember_search();
    }

    /// Called every frame while on the search page: debounces typing before issuing a search.
    pub(super) fn drive_search(&mut self) {
        if self.search.edit.text != self.search.seen {
            self.search.seen.clone_from(&self.search.edit.text);
            self.search.changed_at = self.now;
            return;
        }
        if self.search.edit.text.trim() != self.search.query && self.now - self.search.changed_at > 0.30 && self.session != Session::LoggedOut {
            let text = self.search.edit.text.clone();
            self.run_search(text);
        }
    }

    pub fn is_liked(&self, id: &str) -> bool {
        self.liked.contains(id)
    }

    pub fn toggle_like(&mut self, track: &Track) {
        let like = !self.liked.contains(&track.id);
        self.set_like(track, like);
    }

    pub fn set_like(&mut self, track: &Track, like: bool) {
        if like {
            self.liked.insert(track.id.clone());
            if let Load::Ready(v) = &mut self.library.tracks
                && !v.iter().any(|t| t.id == track.id)
            {
                v.insert(0, track.clone());
            }
        } else {
            self.liked.remove(&track.id);
            if let Load::Ready(v) = &mut self.library.tracks {
                v.retain(|t| t.id != track.id);
            }
        }
        let (b, uid, t) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid), track.clone());
        self.job(
            async move {
                let res = b.set_like(uid, &t, like).await;
                (t, res)
            },
            move |s, _, (t, res)| {
                if let Err(e) = res {
                    if like {
                        s.liked.remove(&t.id);
                        if let Load::Ready(v) = &mut s.library.tracks {
                            v.retain(|x| x.id != t.id);
                        }
                    } else {
                        s.liked.insert(t.id.clone());
                        if let Load::Ready(v) = &mut s.library.tracks
                            && !v.iter().any(|x| x.id == t.id)
                        {
                            v.insert(0, t);
                        }
                    }
                    s.error_toast(&e);
                }
            },
        );
    }

    /// Reports a dislike to Yandex, drops any existing like, and skips if this track is playing.
    pub fn dislike(&mut self, track: &Track) {
        if self.liked.contains(&track.id) {
            self.set_like(track, false);
        }
        let (b, uid, t) = (self.backend.clone(), self.account.as_ref().map_or(0, |a| a.uid), track.clone());
        self.job(async move { b.set_dislike(uid, &t).await }, |s, _, res| {
            if let Err(e) = res {
                s.error_toast(&e);
            }
        });
        let text = self.tr(S::Disliked);
        self.toast(text);
        if self.current().is_some_and(|c| c.id == track.id) {
            self.next();
        }
    }
}
