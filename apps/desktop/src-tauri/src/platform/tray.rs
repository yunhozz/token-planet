use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, LogicalSize, Manager,
};

use crate::growth::WorldSnapshot;

pub const OPEN_ID: &str = "open-world";
pub const QUIT_ID: &str = "quit-world";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    Open,
    Quit,
}

pub fn menu_action(id: &str) -> Option<MenuAction> {
    match id {
        OPEN_ID => Some(MenuAction::Open),
        QUIT_ID => Some(MenuAction::Quit),
        _ => None,
    }
}

pub fn next_window_visibility(currently_visible: bool) -> bool {
    !currently_visible
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_size(LogicalSize::new(390.0, 700.0));
        let _ = window.center();
        let _ = app.emit("show-compact", ());
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        if next_window_visibility(visible) {
            show_window(app);
        } else {
            let _ = window.hide();
        }
    }
}

pub fn install(app: &mut App) -> tauri::Result<()> {
    let menu = build_menu(app.handle(), None)?;
    TrayIconBuilder::with_id("token-world")
        .icon(
            app.default_window_icon()
                .expect("application icon configured")
                .clone(),
        )
        .tooltip("Token World")
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match menu_action(event.id.as_ref()) {
            Some(MenuAction::Open) => show_window(app),
            Some(MenuAction::Quit) => app.exit(0),
            None => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn refresh_status(app: &AppHandle, snapshot: &WorldSnapshot) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id("token-world") {
        tray.set_menu(Some(build_menu(app, Some(snapshot))?))?;
    }
    Ok(())
}

fn build_menu(
    app: &AppHandle,
    snapshot: Option<&WorldSnapshot>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, OPEN_ID, "Token World 열기", true, None::<&str>)?;
    let status = match snapshot {
        Some(world) => format!(
            "수집 상태: Codex {} / Claude Code {}",
            label(world.usage.codex.coverage),
            label(world.usage.claude_code.coverage)
        ),
        None => "수집 상태: 확인 중".to_string(),
    };
    let source = MenuItem::with_id(app, "source-status", status, false, None::<&str>)?;
    let sync = MenuItem::with_id(
        app,
        "sync-status",
        "동기화: 이 기기에서만 저장 중",
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, QUIT_ID, "Token World 종료", true, None::<&str>)?;
    Menu::with_items(app, &[&open, &source, &sync, &quit])
}

fn label(coverage: crate::domain::usage::UsageCoverage) -> &'static str {
    use crate::domain::usage::UsageCoverage;
    match coverage {
        UsageCoverage::Complete => "확인됨",
        UsageCoverage::Partial => "일부만 확인됨",
        UsageCoverage::Unavailable => "확인 불가",
        UsageCoverage::Unsupported => "형식 확인 필요",
        UsageCoverage::UserDisabled => "사용 안 함",
    }
}

#[cfg(test)]
mod tests {
    use super::{menu_action, next_window_visibility, MenuAction, OPEN_ID, QUIT_ID};

    #[test]
    fn native_menu_ids_map_to_open_and_quit() {
        assert_eq!(menu_action(OPEN_ID), Some(MenuAction::Open));
        assert_eq!(menu_action(QUIT_ID), Some(MenuAction::Quit));
        assert_eq!(menu_action("source-status"), None);
    }

    #[test]
    fn tray_click_toggles_compact_window_and_menu_open_always_shows_it() {
        assert!(!next_window_visibility(true));
        assert!(next_window_visibility(false));
    }
}
