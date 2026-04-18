use uuid::Uuid;

/// Deterministic UUIDs from migration 0005 — keep in sync.
pub const OPERATOR_ORG_ID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000001);
pub const ROLE_OPERATOR_ADMIN: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000101);
pub const ROLE_ORG_OWNER:      Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000102);
pub const ROLE_ORG_ADMIN:      Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000103);
pub const ROLE_ORG_MEMBER:     Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_000000000104);
