use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, SecondsFormat};
use serde::ser::Error as _;
use serde::{de, ser, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

use super::{
    cosmetic_shop::{
        ActiveEffects, GuestShopImportData, GuestShopImportDisposition, ShopEffectTimeline,
        ShopState,
    },
    growth_journal::GrowthJournal,
    planet::{PlanetDeviceContributionSnapshot, PlanetState},
    usage::Agent,
};

const MAX_STORED_INTEGER: u64 = i64::MAX as u64;

#[derive(Debug)]
pub enum GuestImportEncodingError {
    Serialize(serde_json::Error),
    Deserialize(String),
    IntegerOutOfRange,
    InvalidUuid,
    InvalidTimestamp,
    InvalidDate,
    InvalidDigest,
    InvalidFixedValue,
    InvalidCanonicalPayload,
    InvalidResultShape,
    NonFiniteNumber,
}

pub fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, GuestImportEncodingError> {
    validate_serializable(value)?;
    let value = serde_json::to_value(value).map_err(GuestImportEncodingError::Serialize)?;
    // Integer checks apply to integer-typed JSON values; existing typed float fields
    // (for example placement coordinates) keep serde_json's established encoding.
    validate_integer_bounds(&value)?;
    serde_json::to_vec(&value).map_err(GuestImportEncodingError::Serialize)
}

fn validate_integer_bounds(value: &Value) -> Result<(), GuestImportEncodingError> {
    match value {
        Value::Number(number) if number.is_i64() => {
            if number.as_i64().is_some_and(|value| value < 0) {
                return Err(GuestImportEncodingError::IntegerOutOfRange);
            }
        }
        Value::Number(number) if number.is_u64() => {
            if number.as_u64().is_some_and(|value| value > i64::MAX as u64) {
                return Err(GuestImportEncodingError::IntegerOutOfRange);
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_integer_bounds(value)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                validate_integer_bounds(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

impl std::fmt::Display for GuestImportEncodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialize(error) => write!(f, "JSON serialization failed: {error}"),
            Self::Deserialize(error) => write!(f, "JSON deserialization failed: {error}"),
            Self::IntegerOutOfRange => f.write_str("integer is outside the supported range"),
            Self::InvalidUuid => f.write_str("UUID is not canonical lowercase or is nil"),
            Self::InvalidTimestamp => {
                f.write_str("timestamp is not canonical UTC microsecond time")
            }
            Self::InvalidDate => f.write_str("date is not canonical YYYY-MM-DD"),
            Self::InvalidDigest => f.write_str("digest is not lowercase SHA-256 hex"),
            Self::InvalidFixedValue => f.write_str("version or fixed value is unsupported"),
            Self::InvalidCanonicalPayload => f.write_str("canonical payload has unexpected keys"),
            Self::InvalidResultShape => f.write_str("result fields do not match its status"),
            Self::NonFiniteNumber => f.write_str("non-finite JSON number is unsupported"),
        }
    }
}

impl std::error::Error for GuestImportEncodingError {}

impl ser::Error for GuestImportEncodingError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self::Serialize(<serde_json::Error as ser::Error>::custom(message))
    }
}

#[derive(Clone, Copy)]
struct ValidationSerializer;

fn validate_serializable<T: Serialize + ?Sized>(value: &T) -> Result<(), GuestImportEncodingError> {
    value.serialize(ValidationSerializer)
}

fn validate_signed_integer(value: i128) -> Result<(), GuestImportEncodingError> {
    if value < 0 || value > i64::MAX as i128 {
        return Err(GuestImportEncodingError::IntegerOutOfRange);
    }
    Ok(())
}

fn validate_unsigned_integer(value: u128) -> Result<(), GuestImportEncodingError> {
    if value > i64::MAX as u128 {
        return Err(GuestImportEncodingError::IntegerOutOfRange);
    }
    Ok(())
}

struct ValidationCompound;

macro_rules! impl_validation_sequence {
    ($trait_name:ident, $method_name:ident) => {
        impl ser::$trait_name for ValidationCompound {
            type Ok = ();
            type Error = GuestImportEncodingError;

            fn $method_name<T: Serialize + ?Sized>(
                &mut self,
                value: &T,
            ) -> Result<(), Self::Error> {
                validate_serializable(value)
            }

            fn end(self) -> Result<Self::Ok, Self::Error> {
                Ok(())
            }
        }
    };
}

impl_validation_sequence!(SerializeSeq, serialize_element);
impl_validation_sequence!(SerializeTuple, serialize_element);
impl_validation_sequence!(SerializeTupleStruct, serialize_field);

impl ser::SerializeTupleVariant for ValidationCompound {
    type Ok = ();
    type Error = GuestImportEncodingError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        validate_serializable(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl ser::SerializeMap for ValidationCompound {
    type Ok = ();
    type Error = GuestImportEncodingError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        validate_serializable(key)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        validate_serializable(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl ser::SerializeStruct for ValidationCompound {
    type Ok = ();
    type Error = GuestImportEncodingError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        validate_serializable(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for ValidationCompound {
    type Ok = ();
    type Error = GuestImportEncodingError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        validate_serializable(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl ser::Serializer for ValidationSerializer {
    type Ok = ();
    type Error = GuestImportEncodingError;
    type SerializeSeq = ValidationCompound;
    type SerializeTuple = ValidationCompound;
    type SerializeTupleStruct = ValidationCompound;
    type SerializeTupleVariant = ValidationCompound;
    type SerializeMap = ValidationCompound;
    type SerializeStruct = ValidationCompound;
    type SerializeStructVariant = ValidationCompound;

    fn serialize_bool(self, _: bool) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i8(self, value: i8) -> Result<(), Self::Error> {
        validate_signed_integer(value as i128)
    }
    fn serialize_i16(self, value: i16) -> Result<(), Self::Error> {
        validate_signed_integer(value as i128)
    }
    fn serialize_i32(self, value: i32) -> Result<(), Self::Error> {
        validate_signed_integer(value as i128)
    }
    fn serialize_i64(self, value: i64) -> Result<(), Self::Error> {
        validate_signed_integer(value as i128)
    }
    fn serialize_i128(self, value: i128) -> Result<(), Self::Error> {
        validate_signed_integer(value)
    }
    fn serialize_u8(self, value: u8) -> Result<(), Self::Error> {
        validate_unsigned_integer(value as u128)
    }
    fn serialize_u16(self, value: u16) -> Result<(), Self::Error> {
        validate_unsigned_integer(value as u128)
    }
    fn serialize_u32(self, value: u32) -> Result<(), Self::Error> {
        validate_unsigned_integer(value as u128)
    }
    fn serialize_u64(self, value: u64) -> Result<(), Self::Error> {
        validate_unsigned_integer(value as u128)
    }
    fn serialize_u128(self, value: u128) -> Result<(), Self::Error> {
        validate_unsigned_integer(value)
    }
    fn serialize_f32(self, value: f32) -> Result<(), Self::Error> {
        if value.is_finite() {
            Ok(())
        } else {
            Err(GuestImportEncodingError::NonFiniteNumber)
        }
    }
    fn serialize_f64(self, value: f64) -> Result<(), Self::Error> {
        if value.is_finite() {
            Ok(())
        } else {
            Err(GuestImportEncodingError::NonFiniteNumber)
        }
    }
    fn serialize_char(self, _: char) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_str(self, _: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_none(self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Self::Error> {
        validate_serializable(value)
    }
    fn serialize_unit(self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        validate_serializable(value)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        validate_serializable(value)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(ValidationCompound)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(ValidationCompound)
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(ValidationCompound)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(ValidationCompound)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(ValidationCompound)
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(ValidationCompound)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(ValidationCompound)
    }
}

fn bounded_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    if value > MAX_STORED_INTEGER {
        return Err(de::Error::custom("integer exceeds signed bigint range"));
    }
    Ok(value)
}

fn positive_bounded_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = bounded_u64(deserializer)?;
    if value == 0 {
        return Err(de::Error::custom("integer must be positive"));
    }
    Ok(value)
}

fn deserialize_fixed_version<'de, D>(deserializer: D, expected: u8) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u8::deserialize(deserializer)?;
    if value != expected {
        return Err(de::Error::custom("unsupported version"));
    }
    Ok(value)
}

fn deserialize_v1<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_fixed_version(deserializer, 1)
}

fn deserialize_v2<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_fixed_version(deserializer, 2)
}

fn checked_uuid(value: String) -> Result<String, GuestImportEncodingError> {
    let parsed =
        uuid::Uuid::parse_str(&value).map_err(|_| GuestImportEncodingError::InvalidUuid)?;
    if parsed.is_nil() || parsed.to_string() != value {
        return Err(GuestImportEncodingError::InvalidUuid);
    }
    Ok(value)
}

fn deserialize_uuid<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    checked_uuid(String::deserialize(deserializer)?).map_err(de::Error::custom)
}

fn deserialize_account_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    let Some(id) = value.strip_prefix("account:") else {
        return Err(de::Error::custom("account id must use account:<uuid>"));
    };
    checked_uuid(id.to_string()).map_err(de::Error::custom)?;
    Ok(value)
}

fn deserialize_local_source<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value != "local" {
        return Err(de::Error::custom("source account must be local"));
    }
    Ok(value)
}

fn checked_timestamp(value: String) -> Result<String, GuestImportEncodingError> {
    let parsed = DateTime::parse_from_rfc3339(&value)
        .map_err(|_| GuestImportEncodingError::InvalidTimestamp)?;
    if parsed.to_rfc3339_opts(SecondsFormat::Micros, true) != value {
        return Err(GuestImportEncodingError::InvalidTimestamp);
    }
    Ok(value)
}

fn deserialize_timestamp<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    checked_timestamp(String::deserialize(deserializer)?).map_err(de::Error::custom)
}

fn deserialize_optional_timestamp<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(checked_timestamp)
        .transpose()
        .map_err(de::Error::custom)
}

fn deserialize_date<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    let date = NaiveDate::parse_from_str(&value, "%Y-%m-%d")
        .map_err(|_| de::Error::custom("invalid date"))?;
    if date.format("%Y-%m-%d").to_string() != value {
        return Err(de::Error::custom("date is not canonical"));
    }
    Ok(value)
}

fn checked_digest(value: String) -> Result<String, GuestImportEncodingError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(GuestImportEncodingError::InvalidDigest);
    }
    Ok(value)
}

fn deserialize_digest<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    checked_digest(String::deserialize(deserializer)?).map_err(de::Error::custom)
}

fn deserialize_uuid_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|value| checked_uuid(value).map_err(de::Error::custom))
        .collect()
}

fn deserialize_zero<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = bounded_u64(deserializer)?;
    if value != 0 {
        return Err(de::Error::custom("value must be zero"));
    }
    Ok(value)
}

fn deserialize_canonical_payload<'de, D>(
    deserializer: D,
) -> Result<PlanetDeviceContributionSnapshot, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    validate_canonical_payload_value::<D::Error>(&value)?;
    serde_json::from_value(value).map_err(de::Error::custom)
}

struct GuestShopImportDataV2<'a>(&'a GuestShopImportData);

impl Serialize for GuestShopImportDataV2<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        validate_serializable(self.0).map_err(S::Error::custom)?;
        let mut value = serde_json::to_value(self.0).map_err(S::Error::custom)?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| S::Error::custom("guest import data must be an object"))?;
        object
            .entry("purchase_proofs".to_string())
            .or_insert_with(|| Value::Array(vec![]));
        object
            .entry("removal_proofs".to_string())
            .or_insert_with(|| Value::Array(vec![]));
        value.serialize(serializer)
    }
}

fn serialize_guest_shop_import_data_v2<S>(
    data: &GuestShopImportData,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    GuestShopImportDataV2(data).serialize(serializer)
}

fn deserialize_guest_shop_import_data_v2<'de, D>(
    deserializer: D,
) -> Result<GuestShopImportData, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let object = value
        .as_object()
        .ok_or_else(|| de::Error::custom("guest import data must be an object"))?;
    for proof_array in ["purchase_proofs", "removal_proofs"] {
        if !object.get(proof_array).is_some_and(Value::is_array) {
            return Err(de::Error::custom(format!(
                "schema 2 guest import data requires {proof_array} array"
            )));
        }
    }
    serde_json::from_value(value).map_err(de::Error::custom)
}

fn validate_exact_object_keys<E: de::Error>(value: &Value, keys: &[&str]) -> Result<(), E> {
    let Some(object) = value.as_object() else {
        return Err(E::custom("expected an object"));
    };
    if object.len() != keys.len() || object.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(E::custom("object has unexpected keys"));
    }
    Ok(())
}

fn validate_canonical_payload_value<E: de::Error>(value: &Value) -> Result<(), E> {
    const KEYS: [&str; 9] = [
        "activity_days",
        "canonical_version",
        "current_cycle_id",
        "current_planet_tokens",
        "daily_segments",
        "daily_tokens",
        "device_id",
        "incomplete",
        "lifetime_tokens",
    ];
    validate_exact_object_keys::<E>(value, &KEYS)?;
    let object = value.as_object().expect("validated object");
    for segment in object
        .get("daily_segments")
        .and_then(Value::as_array)
        .ok_or_else(|| E::custom("daily_segments must be an array"))?
    {
        validate_exact_object_keys::<E>(
            segment,
            &["cycle_id", "date", "effect_revision", "tokens"],
        )?;
    }
    for activity_day in object
        .get("activity_days")
        .and_then(Value::as_array)
        .ok_or_else(|| E::custom("activity_days must be an array"))?
    {
        validate_exact_object_keys::<E>(
            activity_day,
            &["cycle_id", "first_occurred_at_utc", "reward_date", "tokens"],
        )?;
    }
    Ok(())
}

fn deserialize_optional_canonical_payload<'de, D>(
    deserializer: D,
) -> Result<Option<PlanetDeviceContributionSnapshot>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Value>::deserialize(deserializer)?
        .map(|value| {
            validate_canonical_payload_value::<D::Error>(&value)?;
            serde_json::from_value(value).map_err(de::Error::custom)
        })
        .transpose()
}

fn validate_planet_state_value<E: de::Error>(value: &Value) -> Result<(), E> {
    validate_exact_object_keys::<E>(
        value,
        &[
            "version",
            "profile",
            "timezone",
            "current_cycle_id",
            "cycle_started_at_utc",
            "last_reset_at_utc",
            "wallet_balance",
            "wallet_credits",
            "current_planet_tokens",
            "lifetime_tokens",
            "growth_credit",
            "stage",
            "progress_to_next",
            "incomplete",
            "can_reset",
            "reset_available_at_utc",
            "objects",
            "removed_natural_keys",
        ],
    )?;
    let object = value.as_object().expect("validated object");
    if let Some(profile) = object.get("profile").filter(|value| !value.is_null()) {
        validate_exact_object_keys::<E>(profile, &["nickname", "avatar"])?;
    }
    for credit in object
        .get("wallet_credits")
        .and_then(Value::as_array)
        .ok_or_else(|| E::custom("wallet_credits must be an array"))?
    {
        validate_exact_object_keys::<E>(
            credit,
            &["previous_cycle_id", "amount", "created_at_utc"],
        )?;
    }
    for planet_object in object
        .get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| E::custom("objects must be an array"))?
    {
        validate_exact_object_keys::<E>(
            planet_object,
            &["stage", "ordinal", "kind", "x", "y", "seed"],
        )?;
    }
    for key in object
        .get("removed_natural_keys")
        .and_then(Value::as_array)
        .ok_or_else(|| E::custom("removed_natural_keys must be an array"))?
    {
        validate_exact_object_keys::<E>(key, &["cycle_id", "stage", "ordinal"])?;
    }
    Ok(())
}

fn deserialize_optional_planet_state<'de, D>(
    deserializer: D,
) -> Result<Option<PlanetState>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Value>::deserialize(deserializer)?
        .map(|value| {
            validate_planet_state_value::<D::Error>(&value)?;
            serde_json::from_value(value).map_err(de::Error::custom)
        })
        .transpose()
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestProvenanceDomain {
    OrdinaryFirstResetZeroEffectV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestProvenanceAuthority {
    ClientSelfReported,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestClockPolicy {
    GuestUtcMonotonicV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestOccurrenceCoverage {
    Complete,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopImportV2Request {
    #[serde(deserialize_with = "deserialize_v2")]
    pub schema_version: u8,
    pub snapshot: GuestShopImportV2Snapshot,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopImportV2Snapshot {
    #[serde(deserialize_with = "deserialize_uuid")]
    pub import_id: String,
    #[serde(deserialize_with = "deserialize_account_id")]
    pub target_account_id: String,
    #[serde(deserialize_with = "deserialize_local_source")]
    pub source_account_id: String,
    #[serde(deserialize_with = "deserialize_digest")]
    pub source_fingerprint: String,
    pub disposition: GuestShopImportDisposition,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub captured_at_utc: String,
    pub provenance: GuestProvenanceV1,
    #[serde(deserialize_with = "deserialize_canonical_payload")]
    pub canonical_payload: PlanetDeviceContributionSnapshot,
    #[serde(
        serialize_with = "serialize_guest_shop_import_data_v2",
        deserialize_with = "deserialize_guest_shop_import_data_v2"
    )]
    pub data: GuestShopImportData,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestOccurrence {
    #[serde(deserialize_with = "deserialize_uuid")]
    pub occurrence_id: String,
    #[serde(deserialize_with = "positive_bounded_u64")]
    pub record_version: u64,
    #[serde(deserialize_with = "positive_bounded_u64")]
    pub ingest_seq: u64,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub device_id: String,
    pub agent: Agent,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub occurred_at_utc: String,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub ingested_at_utc: String,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub cycle_id: String,
    #[serde(deserialize_with = "bounded_u64")]
    pub total_tokens: u64,
    pub coverage: GuestOccurrenceCoverage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestProvenanceCycleBound {
    #[serde(deserialize_with = "deserialize_uuid")]
    pub cycle_id: String,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub started_at_utc: String,
    #[serde(deserialize_with = "deserialize_optional_timestamp")]
    pub ended_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestProvenanceBaseline {
    #[serde(deserialize_with = "deserialize_uuid")]
    pub cycle_id: String,
    #[serde(deserialize_with = "deserialize_zero")]
    pub revision: u64,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub started_at_utc: String,
    #[serde(deserialize_with = "deserialize_optional_timestamp")]
    pub ended_at_utc: Option<String>,
    #[serde(deserialize_with = "deserialize_uuid_list")]
    pub active_instance_ids: Vec<String>,
    pub effects: ActiveEffects,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestFirstResetRequest {
    pub kind: GuestFirstResetRequestKind,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub request_id: String,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub cycle_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestFirstResetRequestKind {
    ResetPlanet,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestFirstResetResult {
    pub status: GuestFirstResetStatus,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub request_id: String,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub previous_cycle_id: String,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub new_cycle_id: String,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub reset_at_utc: String,
    #[serde(deserialize_with = "deserialize_zero")]
    pub final_effect_revision: u64,
    #[serde(deserialize_with = "deserialize_uuid_list")]
    pub final_active_instance_ids: Vec<String>,
    pub final_effects: ActiveEffects,
    #[serde(deserialize_with = "deserialize_optional_timestamp")]
    pub frozen_deadline_before_reset_utc: Option<String>,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub reset_available_at_utc: String,
    #[serde(deserialize_with = "bounded_u64")]
    pub raw_tokens: u64,
    #[serde(deserialize_with = "deserialize_zero")]
    pub bonus_tokens: u64,
    #[serde(deserialize_with = "bounded_u64")]
    pub credited_tokens: u64,
    #[serde(deserialize_with = "bounded_u64")]
    pub shop_state_revision: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestFirstResetStatus {
    Reset,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestFirstResetReceipt {
    pub request: GuestFirstResetRequest,
    pub result: GuestFirstResetResult,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestProvenanceV1 {
    #[serde(deserialize_with = "deserialize_v1")]
    pub version: u8,
    pub domain: GuestProvenanceDomain,
    pub authority: GuestProvenanceAuthority,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub lineage_id: String,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub device_id: String,
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub activated_at_utc: String,
    pub clock_policy: GuestClockPolicy,
    #[serde(deserialize_with = "bounded_u64")]
    pub ingest_watermark: u64,
    #[serde(deserialize_with = "bounded_u64")]
    pub occurrence_count: u64,
    #[serde(deserialize_with = "deserialize_digest")]
    pub prefix_fingerprint: String,
    pub occurrences: Vec<GuestOccurrence>,
    pub cycle_bounds: Vec<GuestProvenanceCycleBound>,
    pub baselines: Vec<GuestProvenanceBaseline>,
    pub reset_receipt: GuestFirstResetReceipt,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestJournalLogicalKey {
    #[serde(deserialize_with = "deserialize_uuid")]
    pub device_id: String,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub cycle_id: String,
    #[serde(deserialize_with = "deserialize_date")]
    pub bucket_date: String,
    pub agent: Agent,
    #[serde(deserialize_with = "bounded_u64")]
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestImportAckJournalEntry {
    pub logical_key: GuestJournalLogicalKey,
    #[serde(deserialize_with = "bounded_u64")]
    pub revision: u64,
    #[serde(deserialize_with = "deserialize_digest")]
    pub payload_hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestImportAck {
    #[serde(deserialize_with = "deserialize_uuid")]
    pub lineage_id: String,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub device_id: String,
    #[serde(deserialize_with = "bounded_u64")]
    pub ingest_watermark: u64,
    #[serde(deserialize_with = "bounded_u64")]
    pub occurrence_count: u64,
    #[serde(deserialize_with = "deserialize_digest")]
    pub prefix_fingerprint: String,
    #[serde(deserialize_with = "bounded_u64")]
    pub canonical_version: u64,
    #[serde(deserialize_with = "deserialize_digest")]
    pub canonical_payload_fingerprint: String,
    pub journal_entries: Vec<GuestImportAckJournalEntry>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestImportSourceRelation {
    Exact,
    AppendOnly,
    CapturedPrefixChanged,
    Unverifiable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestImportStatus {
    Imported,
    ActiveAccount,
    SourceUnverifiable,
    RequestConflict,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestImportPhase {
    Captured,
    AttemptStarted,
    Held,
    Imported,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopImportV2Status {
    pub request: GuestShopImportV2Request,
    pub source_relation: GuestImportSourceRelation,
    pub phase: GuestImportPhase,
    pub correction_hold: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestShopImportV2Result {
    #[serde(deserialize_with = "deserialize_v2")]
    pub schema_version: u8,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub import_id: String,
    #[serde(deserialize_with = "deserialize_account_id")]
    pub account_id: String,
    #[serde(deserialize_with = "deserialize_digest")]
    pub source_fingerprint: String,
    pub status: GuestImportStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shop_state: Option<ShopState>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_planet_state"
    )]
    pub planet_state: Option<PlanetState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_timeline: Option<ShopEffectTimeline>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_canonical_payload"
    )]
    pub canonical_contribution: Option<PlanetDeviceContributionSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal_confirmation: Option<GrowthJournal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ack: Option<GuestImportAck>,
}

impl GuestShopImportV2Result {
    pub fn validate_wire_shape(&self) -> Result<(), GuestImportEncodingError> {
        let all_imported = self.shop_state.is_some()
            && self.planet_state.is_some()
            && self.effect_timeline.is_some()
            && self.canonical_contribution.is_some()
            && self.journal_confirmation.is_some()
            && self.ack.is_some();
        let all_omitted = self.shop_state.is_none()
            && self.planet_state.is_none()
            && self.effect_timeline.is_none()
            && self.canonical_contribution.is_none()
            && self.journal_confirmation.is_none()
            && self.ack.is_none();
        if (self.status == GuestImportStatus::Imported && all_imported)
            || (self.status != GuestImportStatus::Imported && all_omitted)
        {
            Ok(())
        } else {
            Err(GuestImportEncodingError::InvalidResultShape)
        }
    }
}

#[derive(Serialize)]
struct PrefixFingerprint<'a> {
    version: u8,
    lineage_id: &'a str,
    device_id: &'a str,
    ingest_watermark: u64,
    occurrences: &'a [GuestOccurrence],
}

pub fn prefix_fingerprint(
    provenance: &GuestProvenanceV1,
) -> Result<String, GuestImportEncodingError> {
    let prefix = PrefixFingerprint {
        version: 1,
        lineage_id: &provenance.lineage_id,
        device_id: &provenance.device_id,
        ingest_watermark: provenance.ingest_watermark,
        occurrences: &provenance.occurrences,
    };
    canonical_json_bytes(&prefix).map(|bytes| digest(&bytes))
}

#[derive(Serialize)]
struct SnapshotFingerprint<'a> {
    import_id: &'a str,
    target_account_id: &'a str,
    source_account_id: &'a str,
    disposition: GuestShopImportDisposition,
    captured_at_utc: &'a str,
    provenance: &'a GuestProvenanceV1,
    canonical_payload: &'a PlanetDeviceContributionSnapshot,
    data: GuestShopImportDataV2<'a>,
}

pub fn source_fingerprint(
    snapshot: &GuestShopImportV2Snapshot,
) -> Result<String, GuestImportEncodingError> {
    let value = SnapshotFingerprint {
        import_id: &snapshot.import_id,
        target_account_id: &snapshot.target_account_id,
        source_account_id: &snapshot.source_account_id,
        disposition: snapshot.disposition,
        captured_at_utc: &snapshot.captured_at_utc,
        provenance: &snapshot.provenance,
        canonical_payload: &snapshot.canonical_payload,
        data: GuestShopImportDataV2(&snapshot.data),
    };
    canonical_json_bytes(&value).map(|bytes| digest(&bytes))
}

enum StrictJsonValue {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<StrictJsonValue>),
    Object(BTreeMap<String, StrictJsonValue>),
}

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StrictJsonVisitor;

        impl<'de> de::Visitor<'de> for StrictJsonVisitor {
            type Value = StrictJsonValue;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON without duplicate object keys or out-of-range integers")
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictJsonValue::Null)
            }

            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictJsonValue::Null)
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StrictJsonValue::Bool(value))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                if value < 0 {
                    return Err(E::custom("negative JSON integer is unsupported"));
                }
                Ok(StrictJsonValue::Number(Number::from(value)))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                if value > MAX_STORED_INTEGER {
                    return Err(E::custom("JSON integer exceeds signed bigint range"));
                }
                Ok(StrictJsonValue::Number(Number::from(value)))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Number::from_f64(value)
                    .map(StrictJsonValue::Number)
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(StrictJsonValue::String(value.to_owned()))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(StrictJsonValue::String(value))
            }

            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(StrictJsonValue::Array(values))
            }

            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, StrictJsonValue>()? {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(de::Error::custom(format!("duplicate object key {key}")));
                    }
                }
                Ok(StrictJsonValue::Object(values))
            }
        }

        deserializer.deserialize_any(StrictJsonVisitor)
    }
}

impl StrictJsonValue {
    fn into_value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => Value::Number(value),
            Self::String(value) => Value::String(value),
            Self::Array(values) => Value::Array(values.into_iter().map(Self::into_value).collect()),
            Self::Object(values) => Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, value.into_value()))
                    .collect::<Map<_, _>>(),
            ),
        }
    }
}

pub fn parse_guest_import_json<T: for<'de> Deserialize<'de>>(
    json: &str,
) -> Result<T, GuestImportEncodingError> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let value = StrictJsonValue::deserialize(&mut deserializer)
        .map_err(|error| GuestImportEncodingError::Deserialize(error.to_string()))?
        .into_value();
    deserializer
        .end()
        .map_err(|error| GuestImportEncodingError::Deserialize(error.to_string()))?;
    serde_json::from_value(value)
        .map_err(|error| GuestImportEncodingError::Deserialize(error.to_string()))
}

pub fn parse_guest_shop_import_v2_request(
    json: &str,
) -> Result<GuestShopImportV2Request, GuestImportEncodingError> {
    parse_guest_import_json(json)
}

pub fn parse_guest_shop_import_v2_result(
    json: &str,
) -> Result<GuestShopImportV2Result, GuestImportEncodingError> {
    let result: GuestShopImportV2Result = parse_guest_import_json(json)?;
    result.validate_wire_shape()?;
    Ok(result)
}

#[cfg(test)]
mod guest_import_v2_types_tests {
    use std::collections::BTreeMap;

    use serde::Serialize;
    use serde_json::json;

    use super::*;

    #[derive(Serialize)]
    struct DeliberatelyUnsorted {
        z: u8,
        items: Vec<u8>,
        a: u8,
    }

    #[derive(Serialize)]
    struct IntegerValue {
        value: u64,
    }

    fn occurrence_json() -> String {
        r#"{"occurrence_id":"44444444-4444-4444-8444-444444444444","record_version":1,"ingest_seq":1,"device_id":"22222222-2222-4222-8222-222222222222","agent":"codex","occurred_at_utc":"2026-10-03T11:59:59.000000Z","ingested_at_utc":"2026-10-03T12:00:00.000000Z","cycle_id":"33333333-3333-4333-8333-333333333333","total_tokens":42,"coverage":"complete"}"#.to_string()
    }

    fn provenance_fixture() -> GuestProvenanceV1 {
        let occurrence = parse_guest_import_json(&occurrence_json()).unwrap();
        GuestProvenanceV1 {
            version: 1,
            domain: GuestProvenanceDomain::OrdinaryFirstResetZeroEffectV1,
            authority: GuestProvenanceAuthority::ClientSelfReported,
            lineage_id: "11111111-1111-4111-8111-111111111111".to_string(),
            device_id: "22222222-2222-4222-8222-222222222222".to_string(),
            activated_at_utc: "2026-10-03T11:00:00.000000Z".to_string(),
            clock_policy: GuestClockPolicy::GuestUtcMonotonicV1,
            ingest_watermark: 1,
            occurrence_count: 1,
            prefix_fingerprint: "0".repeat(64),
            occurrences: vec![occurrence],
            cycle_bounds: vec![],
            baselines: vec![],
            reset_receipt: GuestFirstResetReceipt {
                request: GuestFirstResetRequest {
                    kind: GuestFirstResetRequestKind::ResetPlanet,
                    request_id: "55555555-5555-4555-8555-555555555555".to_string(),
                    cycle_id: "33333333-3333-4333-8333-333333333333".to_string(),
                },
                result: GuestFirstResetResult {
                    status: GuestFirstResetStatus::Reset,
                    request_id: "55555555-5555-4555-8555-555555555555".to_string(),
                    previous_cycle_id: "33333333-3333-4333-8333-333333333333".to_string(),
                    new_cycle_id: "66666666-6666-4666-8666-666666666666".to_string(),
                    reset_at_utc: "2026-10-03T12:00:00.000000Z".to_string(),
                    final_effect_revision: 0,
                    final_active_instance_ids: vec![],
                    final_effects: ActiveEffects::default(),
                    frozen_deadline_before_reset_utc: None,
                    reset_available_at_utc: "2026-10-04T12:00:00.000000Z".to_string(),
                    raw_tokens: 42,
                    bonus_tokens: 0,
                    credited_tokens: 42,
                    shop_state_revision: 1,
                },
            },
        }
    }

    fn empty_guest_data() -> GuestShopImportData {
        let mut value = json!({
            "world_timezone": "UTC",
            "planet_timezone": "UTC",
            "reward_timezone": "UTC",
            "planet_device_id": "22222222-2222-4222-8222-222222222222",
            "shop_state_revision": 0,
            "profile": null,
            "activation_at_utc": "2026-10-03T11:00:00.000000Z",
            "current_cycle": {
                "cycle_id": "66666666-6666-4666-8666-666666666666",
                "started_at_utc": "2026-10-03T12:00:00.000000Z",
                "ended_at_utc": null,
                "is_current": true,
                "settled_bonus_tokens": 0
            },
            "historical_cycles": [],
            "last_reset_at_utc": "2026-10-03T12:00:00.000000Z",
            "reset_available_at_utc": "2026-10-04T12:00:00.000000Z",
            "effect_timeline_state": null,
            "effect_cycle_bounds_authoritative": false,
            "contribution_canonical_version": 1,
            "natural_objects": [],
            "landscape_instances": [],
            "placements": [],
            "landscape_edit_versions": [],
            "avatar_owned": [],
            "avatar_equipment": [],
            "cosmetic_equipment": [],
            "pending_purchases": [],
            "cosmetic_purchases": [],
            "purchases": [],
            "purchase_proofs": [],
            "natural_removals": [],
            "removal_debits": [],
            "removal_proofs": [],
            "effect_history": [],
            "effect_cycle_bounds": [],
            "effect_contributions": [],
            "activity_days": [],
            "game_rewards": [],
        });
        let rest = json!({
            "wallet_credits": [],
            "unverified_planet_wallet_claims": [],
            "cycle_settlements": [],
            "era_progress": [],
            "daily_agent_totals": [],
            "usage_aggregates": [],
            "lifetime_usage_tokens": 42,
            "current_cycle_usage_tokens": 0,
            "cycle_usage_totals": [],
            "growth_journal_state": null,
            "growth_journal_cycles": [],
            "growth_journal_entries": [],
            "reset_settlement_proofs": [],
            "integrity_issues": [],
            "reset_receipts_unverifiable": false,
            "legacy_partial_import_pending": false
        });
        value
            .as_object_mut()
            .unwrap()
            .extend(rest.as_object().unwrap().clone());
        serde_json::from_value(value).unwrap()
    }

    fn snapshot_fixture() -> GuestShopImportV2Snapshot {
        use super::super::planet::PlanetDeviceContribution;

        GuestShopImportV2Snapshot {
            import_id: "77777777-7777-4777-8777-777777777777".to_string(),
            target_account_id: "account:88888888-8888-4888-8888-888888888888".to_string(),
            source_account_id: "local".to_string(),
            source_fingerprint: "0".repeat(64),
            disposition: GuestShopImportDisposition::LocalIntegrityValidated,
            captured_at_utc: "2026-10-03T12:01:00.000000Z".to_string(),
            provenance: provenance_fixture(),
            canonical_payload: PlanetDeviceContributionSnapshot {
                raw: PlanetDeviceContribution {
                    device_id: "22222222-2222-4222-8222-222222222222".to_string(),
                    current_cycle_id: "66666666-6666-4666-8666-666666666666".to_string(),
                    lifetime_tokens: 42,
                    current_planet_tokens: 0,
                    daily_tokens: BTreeMap::new(),
                    incomplete: false,
                },
                canonical_version: 1,
                daily_segments: vec![],
                activity_days: vec![],
            },
            data: empty_guest_data(),
        }
    }

    #[test]
    fn canonical_json_sorts_object_keys_without_reordering_arrays() {
        let value = DeliberatelyUnsorted {
            z: 1,
            items: vec![2, 1],
            a: 2,
        };
        assert_eq!(
            canonical_json_bytes(&value).unwrap(),
            br#"{"a":2,"items":[2,1],"z":1}"#
        );
    }

    #[test]
    fn canonical_json_rejects_integers_above_signed_bigint_max() {
        assert!(canonical_json_bytes(&IntegerValue {
            value: i64::MAX as u64 + 1,
        })
        .is_err());

        #[derive(Serialize)]
        struct SignedInteger {
            value: i64,
        }
        assert!(canonical_json_bytes(&SignedInteger { value: -1 }).is_err());
    }

    #[test]
    fn canonical_json_rejects_non_finite_numbers() {
        #[derive(Serialize)]
        struct FloatingValue {
            value: f64,
        }
        assert!(canonical_json_bytes(&FloatingValue { value: f64::NAN }).is_err());
    }

    #[test]
    fn canonical_json_preserves_finite_legacy_float_fields() {
        #[derive(Serialize)]
        struct ExistingGrowthFields {
            progress_to_next: f64,
            growth_credit: f64,
        }
        assert_eq!(
            canonical_json_bytes(&ExistingGrowthFields {
                progress_to_next: 0.25,
                growth_credit: 0.5,
            })
            .unwrap(),
            br#"{"growth_credit":0.5,"progress_to_next":0.25}"#
        );
    }

    #[test]
    fn raw_parser_rejects_duplicate_object_keys_at_any_depth() {
        let json = occurrence_json().replace(
            "\"total_tokens\":42",
            "\"total_tokens\":42,\"total_tokens\":43",
        );
        assert!(parse_guest_import_json::<GuestOccurrence>(&json).is_err());
    }

    #[test]
    fn occurrence_wire_rejects_unknown_fields() {
        let json = occurrence_json().replace(
            "\"coverage\":\"complete\"",
            "\"coverage\":\"complete\",\"extra\":true",
        );
        assert!(parse_guest_import_json::<GuestOccurrence>(&json).is_err());
    }

    #[test]
    fn envelope_rejects_unknown_fields() {
        assert!(parse_guest_import_json::<GuestShopImportV2Request>(
            r#"{"schema_version":2,"unexpected":true}"#
        )
        .is_err());
    }

    #[test]
    fn occurrence_rejects_noncanonical_uuid_and_non_microsecond_utc_timestamp() {
        let uppercase_uuid = occurrence_json().replace(
            "44444444-4444-4444-8444-444444444444",
            "44444444-4444-4444-8444-44444444444A",
        );
        assert!(parse_guest_import_json::<GuestOccurrence>(&uppercase_uuid).is_err());

        let nil_uuid = occurrence_json().replace(
            "44444444-4444-4444-8444-444444444444",
            "00000000-0000-0000-0000-000000000000",
        );
        assert!(parse_guest_import_json::<GuestOccurrence>(&nil_uuid).is_err());

        let short_timestamp =
            occurrence_json().replace("2026-10-03T11:59:59.000000Z", "2026-10-03T11:59:59Z");
        assert!(parse_guest_import_json::<GuestOccurrence>(&short_timestamp).is_err());

        let utc_offset = occurrence_json().replace(
            "2026-10-03T11:59:59.000000Z",
            "2026-10-03T11:59:59.000000+00:00",
        );
        assert!(parse_guest_import_json::<GuestOccurrence>(&utc_offset).is_err());

        let invalid_calendar =
            occurrence_json().replace("2026-10-03T11:59:59.000000Z", "2026-02-30T11:59:59.000000Z");
        assert!(parse_guest_import_json::<GuestOccurrence>(&invalid_calendar).is_err());
    }

    #[test]
    fn occurrence_rejects_nonpositive_sequence_and_overflowing_token_count() {
        let zero_sequence = occurrence_json().replace("\"ingest_seq\":1", "\"ingest_seq\":0");
        assert!(parse_guest_import_json::<GuestOccurrence>(&zero_sequence).is_err());

        let too_many_tokens = occurrence_json().replace(
            "\"total_tokens\":42",
            "\"total_tokens\":9223372036854775808",
        );
        assert!(parse_guest_import_json::<GuestOccurrence>(&too_many_tokens).is_err());
    }

    #[test]
    fn occurrence_record_version_is_positive_and_tokens_are_integer_only() {
        let zero_version =
            occurrence_json().replace("\"record_version\":1", "\"record_version\":0");
        assert!(parse_guest_import_json::<GuestOccurrence>(&zero_version).is_err());

        let fractional_tokens =
            occurrence_json().replace("\"total_tokens\":42", "\"total_tokens\":42.5");
        assert!(parse_guest_import_json::<GuestOccurrence>(&fractional_tokens).is_err());

        let negative_tokens =
            occurrence_json().replace("\"total_tokens\":42", "\"total_tokens\":-1");
        assert!(parse_guest_import_json::<GuestOccurrence>(&negative_tokens).is_err());
    }

    #[test]
    fn canonical_payload_rejects_unknown_keys_in_the_reused_builder_shape() {
        let mut snapshot = serde_json::to_value(snapshot_fixture()).unwrap();
        snapshot["canonical_payload"]["unexpected"] = json!(true);
        let request = json!({"schema_version":2,"snapshot":snapshot});
        assert!(parse_guest_import_json::<GuestShopImportV2Request>(&request.to_string()).is_err());
    }

    #[test]
    fn canonical_payload_rejects_unknown_keys_in_nested_builder_arrays() {
        let mut snapshot = serde_json::to_value(snapshot_fixture()).unwrap();
        snapshot["canonical_payload"]["daily_segments"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "cycle_id":"33333333-3333-4333-8333-333333333333",
                "date":"2026-10-03",
                "effect_revision":0,
                "tokens":42,
                "unexpected":true
            }));
        let request = json!({"schema_version":2,"snapshot":snapshot});
        assert!(parse_guest_import_json::<GuestShopImportV2Request>(&request.to_string()).is_err());
    }

    #[test]
    fn guest_data_rejects_unknown_keys_inside_existing_nested_dto() {
        let mut snapshot = serde_json::to_value(snapshot_fixture()).unwrap();
        snapshot["data"]["current_cycle"]["unexpected"] = json!(true);
        let request = json!({"schema_version":2,"snapshot":snapshot});
        assert!(parse_guest_import_json::<GuestShopImportV2Request>(&request.to_string()).is_err());
    }

    #[test]
    fn result_rejects_unknown_planet_state_nested_keys_without_changing_planet_dto() {
        let planet = json!({
            "version":1,
            "profile":{"nickname":"guest","avatar":"masculine","unexpected":true},
            "timezone":"UTC",
            "current_cycle_id":"66666666-6666-4666-8666-666666666666",
            "cycle_started_at_utc":"2026-10-03T12:00:00.000000Z",
            "last_reset_at_utc":null,
            "wallet_balance":0,
            "wallet_credits":[],
            "current_planet_tokens":0,
            "lifetime_tokens":42,
            "growth_credit":0.0,
            "stage":0,
            "progress_to_next":0.0,
            "incomplete":false,
            "can_reset":false,
            "reset_available_at_utc":null,
            "objects":[],
            "removed_natural_keys":[]
        });
        let response = json!({
            "schema_version":2,
            "import_id":"77777777-7777-4777-8777-777777777777",
            "account_id":"account:88888888-8888-4888-8888-888888888888",
            "source_fingerprint":"0000000000000000000000000000000000000000000000000000000000000000",
            "status":"source_unverifiable",
            "planet_state":planet
        });
        assert!(parse_guest_import_json::<GuestShopImportV2Result>(&response.to_string()).is_err());
    }

    #[test]
    fn prefix_fingerprint_hashes_only_the_specified_canonical_prefix() {
        assert_eq!(
            prefix_fingerprint(&provenance_fixture()).unwrap(),
            "d902f0efaa72666b7b1c85d304042c320acb307ed428a8d3051347239757a033"
        );
    }

    #[test]
    fn source_fingerprint_excludes_its_own_field_but_includes_snapshot_content() {
        let mut snapshot = snapshot_fixture();
        let fingerprint = source_fingerprint(&snapshot).unwrap();
        assert_eq!(fingerprint.len(), 64);
        let wire = serde_json::to_string(&snapshot).unwrap();
        assert!(!wire.contains("source_path"));
        assert!(!wire.contains("prompt"));
        assert!(!wire.contains("conversation"));

        snapshot.source_fingerprint = "a".repeat(64);
        assert_eq!(source_fingerprint(&snapshot).unwrap(), fingerprint);

        snapshot.target_account_id = "account:99999999-9999-4999-8999-999999999999".to_string();
        assert_ne!(source_fingerprint(&snapshot).unwrap(), fingerprint);
    }

    #[test]
    fn complete_v2_envelope_round_trips_existing_nested_dtos() {
        let mut snapshot = snapshot_fixture();
        snapshot.source_fingerprint = source_fingerprint(&snapshot).unwrap();
        let request = GuestShopImportV2Request {
            schema_version: 2,
            snapshot,
        };
        let wire = serde_json::to_string(&request).unwrap();
        let parsed = parse_guest_shop_import_v2_request(&wire).unwrap();
        assert_eq!(parsed, request);
        assert!(!wire.contains("source_path"));
        assert!(!wire.contains("conversation"));
    }

    #[test]
    fn v2_empty_proof_arrays_are_serialized_and_required_on_parse() {
        let mut snapshot = snapshot_fixture();
        snapshot.source_fingerprint = source_fingerprint(&snapshot).unwrap();
        let request = GuestShopImportV2Request {
            schema_version: 2,
            snapshot,
        };
        let wire = serde_json::to_value(&request).unwrap();
        assert_eq!(wire["snapshot"]["data"]["purchase_proofs"], json!([]));
        assert_eq!(wire["snapshot"]["data"]["removal_proofs"], json!([]));

        for proof_array in ["purchase_proofs", "removal_proofs"] {
            let mut omitted_array = wire.clone();
            omitted_array["snapshot"]["data"]
                .as_object_mut()
                .unwrap()
                .remove(proof_array);
            assert!(parse_guest_shop_import_v2_request(&omitted_array.to_string()).is_err());
        }

        let legacy_data = serde_json::to_value(empty_guest_data()).unwrap();
        assert!(legacy_data.get("purchase_proofs").is_none());
        assert!(legacy_data.get("removal_proofs").is_none());
    }

    #[test]
    fn held_result_cannot_carry_success_state_or_ack() {
        let result: GuestShopImportV2Result = parse_guest_shop_import_v2_result(
            r#"{"schema_version":2,"import_id":"77777777-7777-4777-8777-777777777777","account_id":"account:88888888-8888-4888-8888-888888888888","source_fingerprint":"0000000000000000000000000000000000000000000000000000000000000000","status":"source_unverifiable"}"#,
        )
        .unwrap();
        assert_eq!(result.status, GuestImportStatus::SourceUnverifiable);
        assert!(result.ack.is_none());
        let incomplete_imported = r#"{"schema_version":2,"import_id":"77777777-7777-4777-8777-777777777777","account_id":"account:88888888-8888-4888-8888-888888888888","source_fingerprint":"0000000000000000000000000000000000000000000000000000000000000000","status":"imported"}"#;
        assert!(parse_guest_shop_import_v2_result(incomplete_imported).is_err());
        let partial_ack = r#"{"schema_version":2,"import_id":"77777777-7777-4777-8777-777777777777","account_id":"account:88888888-8888-4888-8888-888888888888","source_fingerprint":"0000000000000000000000000000000000000000000000000000000000000000","status":"source_unverifiable","ack":{"lineage_id":"11111111-1111-4111-8111-111111111111","device_id":"22222222-2222-4222-8222-222222222222","ingest_watermark":1,"occurrence_count":1,"prefix_fingerprint":"0000000000000000000000000000000000000000000000000000000000000000","canonical_version":1,"canonical_payload_fingerprint":"1111111111111111111111111111111111111111111111111111111111111111","journal_entries":[]}}"#;
        assert!(parse_guest_shop_import_v2_result(partial_ack).is_err());
    }
}
