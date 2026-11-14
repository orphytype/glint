//! The view: the shell (title bar, dock, toasts), the pages and the widgets they share.

pub mod blobs;
mod dock;
mod ds;
mod icons;
pub mod kit;
mod login;
mod menu;
pub mod np;
mod pages;
mod settings;
mod shapes;
mod shell;

pub use pages::wave_title;
pub use shell::view;
