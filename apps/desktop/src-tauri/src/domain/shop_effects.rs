use std::collections::HashSet;

use super::cosmetic_shop::{
    ActiveEffects, EffectContribution, LandscapeInstance, ShopCategory, ShopEffectType, ShopError,
    ShopProduct,
};

const BPS_DENOMINATOR: u128 = 10_000;
const K_TOKENS: f64 = 100_000.0;

fn add_capped(current: &mut u64, value: u64, cap: u64) {
    *current = current.saturating_add(value).min(cap);
}

fn add_capped_bps(current: &mut u16, value: u64, cap: u16) {
    let remaining = u64::from(cap.saturating_sub(*current));
    *current += value.min(remaining) as u16;
}

/// Sum effects from the placed-instance list supplied by the caller and apply
/// the approved cap for each effect independently.
pub fn capped_effects(
    products: &[ShopProduct],
    active_instances: &[LandscapeInstance],
) -> Result<ActiveEffects, ShopError> {
    let mut effects = ActiveEffects::default();
    let mut seen_ids = HashSet::new();
    for instance in active_instances {
        if !seen_ids.insert(instance.instance_id.as_str()) {
            continue;
        }
        let product = products
            .iter()
            .find(|product| product.sku == instance.sku)
            .ok_or(ShopError::InvalidProduct)?;
        if product.category != ShopCategory::Landscape || product.placement_zone.is_none() {
            return Err(ShopError::InvalidProduct);
        }
        let value = product.effect_value;
        match product.effect_type.ok_or(ShopError::InvalidProduct)? {
            ShopEffectType::TokenEarning => add_capped_bps(&mut effects.token_earning_bps, value, 3_000),
            ShopEffectType::CivilizationGrowth => add_capped_bps(&mut effects.civilization_growth_bps, value, 2_000),
            ShopEffectType::ShopDiscount => add_capped_bps(&mut effects.shop_discount_bps, value, 1_500),
            ShopEffectType::ResetCooldown => add_capped_bps(&mut effects.reset_cooldown_bps, value, 2_500),
            ShopEffectType::NaturalRemovalDiscount => add_capped_bps(&mut effects.natural_removal_discount_bps, value, 3_000),
            ShopEffectType::EraReward => add_capped(&mut effects.era_reward_tokens, value, 10_000_000),
            ShopEffectType::StreakReward => add_capped(&mut effects.streak_reward_tokens, value, 500_000),
        }
    }
    Ok(effects)
}

/// Round a purchase price up after applying the capped basis-point discount.
pub fn discounted_price(base: u64, discount_bps: u16) -> Result<u64, ShopError> {
    if discount_bps > 10_000 {
        return Err(ShopError::InvalidDiscount);
    }
    let multiplier = 10_000_u64 - u64::from(discount_bps);
    let numerator = base
        .checked_mul(multiplier)
        .and_then(|value| value.checked_add(9_999))
        .ok_or(ShopError::ArithmeticOverflow)?;
    Ok((numerator / 10_000).max(1))
}

/// Recompute the base daily growth curve plus the contribution-weighted bonus.
pub fn weighted_growth(
    total_tokens: u64,
    segments: &[EffectContribution],
) -> Result<f64, ShopError> {
    if total_tokens == 0 {
        if segments.iter().any(|segment| segment.tokens != 0) {
            return Err(ShopError::InvalidContribution);
        }
        return Ok(0.0);
    }

    let mut segment_tokens = 0_u64;
    let mut weighted_bps_tokens = 0_u128;
    for segment in segments {
        if segment.growth_bps > 2_000 || segment.wallet_bps > 3_000 {
            return Err(ShopError::InvalidContribution);
        }
        segment_tokens = segment_tokens
            .checked_add(segment.tokens)
            .ok_or(ShopError::ArithmeticOverflow)?;
        weighted_bps_tokens = weighted_bps_tokens
            .checked_add(u128::from(segment.tokens) * u128::from(segment.growth_bps))
            .ok_or(ShopError::ArithmeticOverflow)?;
    }
    if segment_tokens > total_tokens {
        return Err(ShopError::InvalidContribution);
    }

    let base = (1.0 + total_tokens as f64 / K_TOKENS).log2();
    let weighted_share = weighted_bps_tokens as f64 / (total_tokens as f64 * BPS_DENOMINATOR as f64);
    Ok(base * (1.0 + weighted_share))
}

/// Keep fractional token earnings across segments and floor only the final sum.
pub fn cycle_token_bonus(segments: &[EffectContribution]) -> Result<u64, ShopError> {
    let weighted_tokens = segments.iter().try_fold(0_u128, |sum, segment| {
        if segment.wallet_bps > 3_000 {
            return Err(ShopError::InvalidContribution);
        }
        let value = u128::from(segment.tokens) * u128::from(segment.wallet_bps);
        sum.checked_add(value).ok_or(ShopError::ArithmeticOverflow)
    })?;
    u64::try_from(weighted_tokens / BPS_DENOMINATOR).map_err(|_| ShopError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::{capped_effects, cycle_token_bonus, discounted_price, weighted_growth};
    use crate::domain::cosmetic_shop::{
        shop_products, EffectContribution, LandscapeInstance, ShopError,
    };

    fn instance(id: &str, sku: &str) -> LandscapeInstance {
        LandscapeInstance {
            instance_id: id.into(),
            sku: sku.into(),
            variation_index: 0,
            seed: format!("seed-{id}"),
            variation_version: 1,
            placement_version: 0,
        }
    }

    fn contribution(tokens: u64, growth_bps: u16, wallet_bps: u16) -> EffectContribution {
        EffectContribution {
            device_id: "device-a".into(),
            cycle_id: "cycle-a".into(),
            date: "2026-10-01".into(),
            effect_revision: 1,
            tokens,
            growth_bps,
            wallet_bps,
        }
    }

    #[test]
    fn active_effects_count_placed_instances_once_and_apply_each_cap() {
        let products = shop_products();
        let mut placed = Vec::new();
        for index in 0..5 {
            placed.push(instance(&format!("lab-{index}"), "land_laboratory"));
            placed.push(instance(&format!("observatory-{index}"), "land_observatory"));
            placed.push(instance(&format!("bazaar-{index}"), "land_bazaar"));
            placed.push(instance(&format!("reservoir-{index}"), "land_reservoir"));
            placed.push(instance(&format!("moonlet-{index}"), "land_moonlets"));
        }
        let effects = capped_effects(&products, &placed).unwrap();
        assert_eq!(effects.civilization_growth_bps, 2_000);
        assert_eq!(effects.shop_discount_bps, 1_500);
        assert_eq!(effects.token_earning_bps, 1_500);
        assert_eq!(effects.era_reward_tokens, 10_000_000);

        let only_placed = capped_effects(&products, &[instance("one", "land_pond")]).unwrap();
        assert_eq!(only_placed.token_earning_bps, 100);
        assert_eq!(only_placed.civilization_growth_bps, 0);
        let duplicate = capped_effects(
            &products,
            &[instance("same", "land_pond"), instance("same", "land_well")],
        )
        .unwrap();
        assert_eq!(duplicate.token_earning_bps, 100);
        assert_eq!(
            capped_effects(&products, &[instance("avatar", "avatar_wings")]),
            Err(ShopError::InvalidProduct)
        );
    }

    #[test]
    fn prices_round_up_and_reject_intermediate_overflow() {
        assert_eq!(discounted_price(5_000_001, 1_500).unwrap(), 4_250_001);
        assert_eq!(discounted_price(1_000, 3_000).unwrap(), 700);
        assert_eq!(discounted_price(1_000, 9_999).unwrap(), 1);
        assert_eq!(discounted_price(0, 0).unwrap(), 1);
        assert_eq!(discounted_price(1_000, 10_001), Err(ShopError::InvalidDiscount));
        assert_eq!(discounted_price(u64::MAX, 1_500), Err(ShopError::ArithmeticOverflow));
    }

    #[test]
    fn growth_is_occurrence_weighted_order_independent_and_zero_safe() {
        let segments = [contribution(50_000, 2_000, 0), contribution(50_000, 0, 0)];
        let growth = weighted_growth(100_000, &segments).unwrap();
        let reverse = weighted_growth(100_000, &segments.into_iter().rev().collect::<Vec<_>>()).unwrap();
        assert!((growth - 1.1).abs() < 1e-12);
        assert!((reverse - growth).abs() < 1e-12);
        assert_eq!(weighted_growth(0, &[]).unwrap(), 0.0);
        assert_eq!(weighted_growth(10, &[contribution(11, 100, 0)]), Err(ShopError::InvalidContribution));
    }

    #[test]
    fn cycle_bonus_floors_only_after_all_fractional_segments_are_added() {
        let segments = (0..100).map(|_| contribution(1, 0, 100)).collect::<Vec<_>>();
        assert_eq!(cycle_token_bonus(&segments).unwrap(), 1);
    }
}
