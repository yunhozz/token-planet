use tauri::Rect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub(crate) fn physical_icon_bounds(rect: Rect, scale_factor: f64) -> Bounds {
    let position = rect.position.to_physical::<i32>(scale_factor);
    let size = rect.size.to_physical::<u32>(scale_factor);
    Bounds {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }
}

pub(crate) fn popup_bounds(
    icon: Option<Bounds>,
    screen: Bounds,
    preferred_width: u32,
    preferred_height: u32,
) -> Bounds {
    const GAP: i64 = 8;
    const INSET: i64 = 12;

    let screen_x = i64::from(screen.x);
    let screen_y = i64::from(screen.y);
    let screen_width = i64::from(screen.width.max(1));
    let screen_height = i64::from(screen.height.max(1));
    let screen_bottom = screen_y + screen_height;
    let horizontal_inset = INSET.min((screen_width.saturating_sub(1)) / 2);
    let vertical_inset = INSET.min((screen_height.saturating_sub(1)) / 2);
    let available_width = (screen_width - horizontal_inset * 2).max(1) as u32;
    let width = preferred_width.min(available_width).max(1);
    let usable_top = screen_y + vertical_inset;
    let usable_bottom = screen_bottom - vertical_inset;
    let available_height = (usable_bottom - usable_top).max(1);
    let desired_height = i64::from(preferred_height.max(1)).min(available_height);
    let (left, top, height) = match icon {
        Some(icon) => {
            let icon_top = i64::from(icon.y);
            let below_top = (icon_top + i64::from(icon.height) + GAP).max(usable_top);
            let below_room = (usable_bottom - below_top).max(0);
            let above_bottom = icon_top - GAP;
            let above_room = (above_bottom - usable_top).max(0);

            let (top, height) = if below_room >= desired_height {
                (below_top, desired_height)
            } else if above_room >= desired_height {
                (above_bottom - desired_height, desired_height)
            } else if below_room >= above_room {
                (below_top, below_room.min(desired_height).max(1))
            } else {
                let height = above_room.min(desired_height).max(1);
                (above_bottom - height, height)
            };

            (
                i64::from(icon.x) + i64::from(icon.width) / 2 - i64::from(width) / 2,
                top,
                height,
            )
        }
        None => (
            screen_x + (screen_width - i64::from(width)) / 2,
            usable_top,
            desired_height,
        ),
    };
    let max_x = screen_x + screen_width - horizontal_inset - i64::from(width);
    let x = left.clamp(screen_x + horizontal_inset, max_x.max(screen_x));
    let height = height.min(available_height).max(1);
    let max_top = usable_bottom - height;
    let top = top.clamp(usable_top, max_top.max(usable_top));

    Bounds {
        x: x as i32,
        y: top.min(i64::from(i32::MAX)).max(i64::from(i32::MIN)) as i32,
        width,
        height: height.min(i64::from(u32::MAX)) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::{physical_icon_bounds, popup_bounds, Bounds};
    use tauri::{LogicalPosition, LogicalSize, Rect};

    fn screen(width: u32, height: u32) -> Bounds {
        Bounds {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    #[test]
    fn places_below_icon() {
        let result = popup_bounds(
            Some(Bounds { x: 1000, y: 0, width: 24, height: 24 }),
            screen(1440, 900),
            400,
            700,
        );
        assert_eq!(result, Bounds { x: 812, y: 32, width: 400, height: 700 });
    }

    #[test]
    fn places_above_bottom_taskbar_icon_when_popup_will_not_fit_below() {
        let result = popup_bounds(
            Some(Bounds { x: 1800, y: 1040, width: 24, height: 32 }),
            screen(1920, 1080),
            400,
            700,
        );

        assert_eq!(result, Bounds { x: 1508, y: 332, width: 400, height: 700 });
    }

    #[test]
    fn clamps_right_edge() {
        let result = popup_bounds(
            Some(Bounds { x: 1420, y: 0, width: 24, height: 24 }),
            screen(1440, 900),
            400,
            700,
        );
        assert_eq!(result.x, 1028);
        assert_eq!(result.y, 32);
    }

    #[test]
    fn respects_monitor_origin() {
        let result = popup_bounds(
            Some(Bounds { x: 1400, y: 0, width: 24, height: 24 }),
            Bounds { x: 1440, y: 0, width: 800, height: 900 },
            400,
            700,
        );
        assert!(result.x >= 1452);
    }

    #[test]
    fn centers_when_icon_missing() {
        let result = popup_bounds(None, screen(1440, 900), 400, 700);
        assert_eq!(result.x, 520);
        assert_eq!(result.y, 12);
    }

    #[test]
    fn shrinks_on_short_screen() {
        let result = popup_bounds(
            Some(Bounds { x: 100, y: 0, width: 24, height: 24 }),
            screen(800, 500),
            400,
            700,
        );
        assert_eq!(result.height, 456);
    }

    #[test]
    fn converts_logical_rect_at_2x() {
        let rect = Rect {
            position: LogicalPosition::new(100.0, 10.0).into(),
            size: LogicalSize::new(24.0, 20.0).into(),
        };
        assert_eq!(
            physical_icon_bounds(rect, 2.0),
            Bounds { x: 200, y: 20, width: 48, height: 40 }
        );
    }
}
