//! The view: the shell (title bar, dock, toasts), the pages and the widgets they share.

pub mod blobs;
mod details;
mod dock;
mod ds;
mod icons;
pub mod kit;
mod login;
mod menu;
pub mod mini;
pub mod np;
mod np_icons;
mod outline;
mod pages;
pub mod settings;
mod shapes;
mod shell;
pub mod splash;

pub use pages::wave_title;
pub use shell::view;
