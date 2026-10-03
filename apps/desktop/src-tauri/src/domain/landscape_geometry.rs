use serde::{Deserialize, Serialize};

use super::cosmetic_shop::{PlacementZone, ShopError, ShopProduct};
use super::planet::PlanetObject;

const GROUND_WIDTH: f64 = 64.0;
const SKY_WIDTH: f64 = 96.0;
const FOOTPRINT_HEIGHT: f64 = 64.0;
const SKY_HEIGHT: f64 = 220.0;
const CELL_WIDTH: f64 = 56.0;
const CELL_HEIGHT: f64 = 50.0;
const CELL_X_ORIGIN: f64 = 26.0;
const CELL_Y_ORIGIN: f64 = 33.0;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandscapePoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandscapeBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

fn finite_bounds(bounds: LandscapeBounds) -> bool {
    [bounds.x, bounds.y, bounds.width, bounds.height]
        .into_iter()
        .all(f64::is_finite)
        && bounds.width > 0.0
        && bounds.height > 0.0
        && (bounds.x + bounds.width).is_finite()
        && (bounds.y + bounds.height).is_finite()
}

fn intersects(left: LandscapeBounds, right: LandscapeBounds) -> bool {
    left.x < right.x + right.width
        && left.x + left.width > right.x
        && left.y < right.y + right.height
        && left.y + left.height > right.y
}

fn reserved_cell(row: usize, column: usize) -> bool {
    matches!(row, 2 | 8)
        || ((3..8).contains(&row) && matches!(column, 0 | 1 | 19 | 20))
        || (column >= 21 && row <= 6)
}

/// Reproduce the main landscape's stable natural-object layout and retain its
/// full generated-object extent, including objects hidden by later tombstones.
pub fn terrain_bounds(objects: &[PlanetObject]) -> LandscapeBounds {
    const EMPTY_WIDTH: f64 = 1_420.0;
    const EMPTY_HEIGHT: f64 = 548.0;
    const COLUMNS: usize = 24;
    const USABLE_COLUMNS: usize = 21;
    const ROWS: usize = 10;
    const CELL_WIDTH: f64 = 56.0;
    const CELL_HEIGHT: f64 = 50.0;
    const X_ORIGIN: f64 = 26.0;
    const Y_ORIGIN: f64 = 33.0;
    let mut ordered = objects.to_vec();
    ordered.sort_by(|left, right| {
        (
            left.stage,
            left.ordinal,
            left.x,
            left.y,
            left.seed,
            &left.kind,
        )
            .cmp(&(
                right.stage,
                right.ordinal,
                right.x,
                right.y,
                right.seed,
                &right.kind,
            ))
    });
    let mut occupied = [false; COLUMNS * ROWS];
    let (mut max_right, mut max_bottom) = (0.0_f64, 0.0_f64);
    for object in ordered {
        let x = f64::from(object.x.min(100));
        let y = f64::from(object.y.min(100));
        let source_column = ((x / 100.0) * USABLE_COLUMNS as f64).floor() as usize;
        let source_row = ((y / 100.0) * ROWS as f64).floor() as usize;
        let balanced_column =
            (object.ordinal as usize * 13 + object.stage as usize * 7) % USABLE_COLUMNS;
        let balanced_row = (object.ordinal as usize * 3 + object.stage as usize * 2) % ROWS;
        let column_offset = ((source_column + object.seed as usize % 3) % 3) as isize - 1;
        let row_offset = ((source_row + (object.seed / 3) as usize % 3) % 3) as isize - 1;
        let modulo = |value: isize, divisor: isize| ((value % divisor) + divisor) % divisor;
        let mut column = modulo(
            balanced_column as isize + column_offset,
            USABLE_COLUMNS as isize,
        ) as usize;
        let mut row = modulo(balanced_row as isize + row_offset, ROWS as isize) as usize;
        loop {
            let index = row * COLUMNS + column;
            if !occupied[index] && !reserved_cell(row, column) {
                occupied[index] = true;
                break;
            }
            let next = (row * COLUMNS + column + 1) % (COLUMNS * ROWS);
            row = next / COLUMNS;
            column = next % COLUMNS;
        }
        let sprite_x = X_ORIGIN + column as f64 * CELL_WIDTH - 4.0;
        let sprite_y = Y_ORIGIN + row as f64 * CELL_HEIGHT - 10.0;
        max_right = max_right.max(sprite_x + 36.0);
        max_bottom = max_bottom.max(sprite_y + 36.0);
    }
    LandscapeBounds {
        x: 0.0,
        y: 0.0,
        width: EMPTY_WIDTH.max(max_right + 24.0),
        height: EMPTY_HEIGHT.max(max_bottom + 24.0),
    }
}

fn crosses_reserved_walkway(footprint: LandscapeBounds) -> bool {
    for row in 0..10 {
        for column in 0..24 {
            if !reserved_cell(row, column) {
                continue;
            }
            let cell = LandscapeBounds {
                x: CELL_X_ORIGIN + column as f64 * CELL_WIDTH,
                y: CELL_Y_ORIGIN + row as f64 * CELL_HEIGHT,
                width: CELL_WIDTH,
                height: CELL_HEIGHT,
            };
            if intersects(footprint, cell) {
                return true;
            }
        }
    }
    false
}

/// Validate the entire fixed-size sprite footprint, not only its anchor point.
/// The point is the footprint's top-left corner in landscape coordinates.
pub fn validate_placement(
    product: &ShopProduct,
    point: LandscapePoint,
    terrain: LandscapeBounds,
) -> Result<(), ShopError> {
    if !finite_bounds(terrain) || !point.x.is_finite() || !point.y.is_finite() {
        return Err(ShopError::InvalidPlacement);
    }
    let zone = product.placement_zone.ok_or(ShopError::InvalidProduct)?;
    let width = match zone {
        PlacementZone::Ground => GROUND_WIDTH,
        PlacementZone::Sky => SKY_WIDTH,
    };
    let footprint = LandscapeBounds {
        x: point.x,
        y: point.y,
        width,
        height: FOOTPRINT_HEIGHT,
    };
    let right = terrain.x + terrain.width;
    let bottom = terrain.y + terrain.height;
    let in_horizontal_bounds = footprint.x >= terrain.x && footprint.x + footprint.width <= right;
    let in_zone = match zone {
        PlacementZone::Ground => {
            in_horizontal_bounds
                && footprint.y >= terrain.y
                && footprint.y + footprint.height <= bottom
        }
        PlacementZone::Sky => {
            in_horizontal_bounds
                && footprint.y >= terrain.y - SKY_HEIGHT
                && footprint.y + footprint.height <= terrain.y
        }
    };
    if !in_zone || (zone == PlacementZone::Ground && crosses_reserved_walkway(footprint)) {
        return Err(ShopError::InvalidPlacement);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_placement, LandscapeBounds, LandscapePoint};
    use crate::domain::cosmetic_shop::{shop_products, ShopError};

    fn terrain() -> LandscapeBounds {
        LandscapeBounds {
            x: 0.0,
            y: 0.0,
            width: 1420.0,
            height: 548.0,
        }
    }

    #[test]
    fn placement_checks_the_whole_sprite_and_reserved_walkways() {
        let products = shop_products();
        let ground = products
            .iter()
            .find(|product| product.sku == "land_pond")
            .unwrap();
        let sky = products
            .iter()
            .find(|product| product.sku == "land_stars")
            .unwrap();
        assert_eq!(
            validate_placement(ground, LandscapePoint { x: 160.0, y: 200.0 }, terrain()),
            Ok(())
        );
        assert_eq!(
            validate_placement(
                ground,
                LandscapePoint {
                    x: 1390.0,
                    y: 500.0
                },
                terrain()
            ),
            Err(ShopError::InvalidPlacement)
        );
        assert_eq!(
            validate_placement(ground, LandscapePoint { x: 160.0, y: 140.0 }, terrain()),
            Err(ShopError::InvalidPlacement)
        );
        assert_eq!(
            validate_placement(ground, LandscapePoint { x: 50.0, y: 200.0 }, terrain()),
            Err(ShopError::InvalidPlacement)
        );
        assert_eq!(
            validate_placement(
                sky,
                LandscapePoint {
                    x: 100.0,
                    y: -100.0
                },
                terrain()
            ),
            Ok(())
        );
        assert_eq!(
            validate_placement(sky, LandscapePoint { x: 100.0, y: -40.0 }, terrain()),
            Err(ShopError::InvalidPlacement)
        );
        assert_eq!(
            validate_placement(
                ground,
                LandscapePoint {
                    x: f64::INFINITY,
                    y: 200.0
                },
                terrain()
            ),
            Err(ShopError::InvalidPlacement)
        );
    }
}
