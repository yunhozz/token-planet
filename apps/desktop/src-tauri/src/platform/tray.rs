use std::sync::atomic::Ordering;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, Rect,
};

use crate::growth::WorldSnapshot;
use crate::{initial_mode, tray_target, AppState, WindowMode};

pub const OPEN_ID: &str = "open-world";
pub const QUIT_ID: &str = "quit-world";

pub fn icon_rect(app: &AppHandle) -> Option<Rect> {
    app.tray_by_id("token-world")
        .and_then(|tray| tray.rect().ok().flatten())
}

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

fn mode(app: &AppHandle) -> WindowMode {
    app.try_state::<AppState>()
        .and_then(|state| state.window_mode.lock().ok().map(|mode| *mode))
        .unwrap_or_else(|| initial_mode(false))
}

fn set_tray_press_pending(app: &AppHandle, pending: bool) {
    if let Some(state) = app.try_state::<AppState>() {
        state.tray_press_pending.store(pending, Ordering::SeqCst);
    }
}

fn show_window(app: &AppHandle, target: WindowMode, icon_rect: Option<Rect>) {
    if crate::transition_window_mode(app, target, icon_rect).is_ok() && target == WindowMode::Popup
    {
        let _ = app.emit("show-compact", ());
    }
}

fn toggle_window(app: &AppHandle, icon_rect: Option<Rect>) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        if let Some(target) = tray_target(mode(app), visible) {
            show_window(app, target, icon_rect);
        } else {
            let _ = window.hide();
        }
    }
}

fn force_open_window(app: &AppHandle) {
    let target = if mode(app) == WindowMode::Setup {
        WindowMode::Setup
    } else {
        WindowMode::Popup
    };
    show_window(app, target, icon_rect(app));
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
            Some(MenuAction::Open) => force_open_window(app),
            Some(MenuAction::Quit) => app.exit(0),
            None => {}
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Down,
                    ..
                } => set_tray_press_pending(app, true),
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    rect,
                    ..
                } => {
                    toggle_window(app, Some(rect));
                    set_tray_press_pending(app, false);
                }
                _ => {}
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
    use super::{menu_action, MenuAction, OPEN_ID, QUIT_ID};
    use crate::{tray_target, WindowMode};

    #[test]
    fn native_menu_ids_map_to_open_and_quit() {
        assert_eq!(menu_action(OPEN_ID), Some(MenuAction::Open));
        assert_eq!(menu_action(QUIT_ID), Some(MenuAction::Quit));
        assert_eq!(menu_action("source-status"), None);
    }

    #[test]
    fn tray_click_toggles_popup_and_switches_from_detail() {
        assert_eq!(tray_target(WindowMode::Popup, true), None);
        assert_eq!(
            tray_target(WindowMode::Popup, false),
            Some(WindowMode::Popup)
        );
        assert_eq!(
            tray_target(WindowMode::Detail, true),
            Some(WindowMode::Popup)
        );
    }
}
