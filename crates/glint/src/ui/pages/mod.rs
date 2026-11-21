//! Main page router: Home, Search, Library, and Album/Playlist/Artist detail views.

mod detail;
mod garden;
mod home;
mod library;
mod search;
mod shared;

use rune::El;

use crate::app::{Page, State};

pub(crate) use detail::HEAD_COVER_R;
pub use garden::bug_spots;
pub use shared::wave_title;

pub fn page(s: &State, w: f32, h: f32) -> El {
    match &s.page {
        Page::Home => home::home(s, w, h),
        Page::Search => search::search(s, w),
        Page::Library => library::library(s, w),
        Page::Album(_) | Page::Playlist(..) | Page::Artist(_) => detail::detail(s, w),
    }
}
