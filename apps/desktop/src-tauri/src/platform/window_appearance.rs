use crate::WindowMode;
#[cfg(any(target_os = "macos", test))]
fn apply_native(
    mode: WindowMode,
    apply_popover: impl FnOnce() -> Result<(), String>,
    mut clear: impl FnMut() -> Result<bool, String>,
) -> Result<(), String> {
    // Each tagged view is removed one at a time by window-vibrancy.
    while clear()? {}
    if mode == WindowMode::Popup { apply_popover() } else { Ok(()) }
}

pub(crate) fn apply(window: &tauri::WebviewWindow, mode: WindowMode) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let native_window = window.clone();
        // with_webview guarantees that raw AppKit handle access occurs on the main thread.
        // Its Result reports dispatch failures; native operation errors are reported in the closure.
        window.with_webview(move |_| {
            use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState};
            let result = apply_native(mode,
                || window_vibrancy::apply_vibrancy(&native_window,
                    NSVisualEffectMaterial::Popover, Some(NSVisualEffectState::Active), Some(16.0))
                    .map_err(|error| error.to_string()),
                || window_vibrancy::clear_vibrancy(&native_window)
                    .map_err(|error| error.to_string()));
            if let Err(error) = result { eprintln!("window material unavailable: {error}"); }
        }).map_err(|_| "window material unavailable".to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, mode);
    Ok(())
}

pub(crate) fn apply_with_rollback(previous: WindowMode, target: WindowMode, mut apply: impl FnMut(WindowMode) -> Result<(), String>) -> Result<(), String> {
    let result = apply(target);
    if result.is_err() { let _ = apply(previous); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_mode_application_restores_previous_material() {
        let mut modes = Vec::new();
        let result = apply_with_rollback(WindowMode::Popup, WindowMode::Detail, |mode| {
            modes.push(mode);
            if mode == WindowMode::Detail { Err("delivery failed".into()) } else { Ok(()) }
        });
        assert_eq!(result, Err("delivery failed".into()));
        assert_eq!(modes, vec![WindowMode::Detail, WindowMode::Popup]);
    }

    #[test]
    fn popup_clears_all_existing_material_before_applying_once() {
        let operations = std::cell::RefCell::new(Vec::new());
        let mut old_views = 2;
        apply_native(WindowMode::Popup,
            || { operations.borrow_mut().push("apply"); Ok(()) },
            || {
                operations.borrow_mut().push("clear");
                if old_views > 0 { old_views -= 1; Ok(true) } else { Ok(false) }
            }).unwrap();
        assert_eq!(*operations.borrow(), vec!["clear", "clear", "clear", "apply"]);
        assert_eq!(old_views, 0);
    }

    #[test]
    fn detail_and_setup_clear_popup_material() {
        for mode in [WindowMode::Detail, WindowMode::Setup] {
            let mut old_views = 2;
            let mut clears = 0;
            apply_native(mode, || panic!("must not apply Popup material"), || {
                clears += 1;
                if old_views > 0 { old_views -= 1; Ok(true) } else { Ok(false) }
            }).unwrap();
            assert_eq!(old_views, 0, "Detail and Setup must remove all material");
            assert_eq!(clears, 3, "clear must continue until no tagged view remains");
        }
    }
}
