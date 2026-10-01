-- Callers hold the account serialization lock before reading confirmed segments.
create function private.shop_cycle_token_bonus(p_user_id uuid, p_cycle_id text)
returns bigint
language plpgsql security invoker stable set search_path = '' as $$
declare
  v_bonus numeric;
begin
  if p_user_id is null or p_cycle_id is null
    or char_length(p_cycle_id) not between 1 and 80
  then
    raise exception 'cycle token bonus scope is invalid' using errcode = '22023';
  end if;

  select floor(coalesce(sum(c.tokens::numeric * c.wallet_bps::numeric), 0::numeric)
    / 10000::numeric)
  into v_bonus
  from private.shop_effect_contribution c
  where c.user_id = p_user_id and c.cycle_id = p_cycle_id;

  if v_bonus > 9223372036854775807::numeric then
    raise exception 'cycle token bonus exceeds bigint range' using errcode = '22003';
  end if;
  return v_bonus::bigint;
end;
$$;

revoke all on function private.shop_cycle_token_bonus(uuid, text)
  from public, anon, authenticated, service_role;
