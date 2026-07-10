//! TUI component modules.
//!
//! Layout-aware widgets that compose the RGoat terminal interface.

pub mod theme;
pub mod status_bar;
pub mod tool_card;
pub mod diff_view;
pub mod approval_dialog;
pub mod work_sidebar;
pub mod markdown;

pub use work_sidebar::{WorkSidebar, WorkSidebarData};

