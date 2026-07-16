pub mod app_profiles;
pub mod audio;
pub mod commands;
pub mod context;
pub mod corrections;
pub mod data_flow;
pub mod hotkey;
pub mod inject;
pub mod local_data;
pub mod mode;
pub mod polish;
pub mod recovery;
pub mod selected_learning;
pub mod selected_voice_edit;
pub mod selection;
pub mod session_service;
pub mod settings;
pub mod speech;
pub mod startup;
pub mod state;
pub mod tray;

// 通常binを二重manifestにせず、lib unit-test executableだけへ
// Common-Controls v6 resource archiveを明示リンクする。
#[cfg(all(test, target_os = "windows"))]
#[link(
    name = "windows_test_manifest_archive",
    kind = "static",
    modifiers = "+whole-archive"
)]
unsafe extern "C" {}
