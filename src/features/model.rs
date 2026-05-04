use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureValueType {
    Bool,
    String,
    Int,
    EnumSet,
    Json,
}

impl FeatureValueType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::String => "string",
            Self::Int => "int",
            Self::EnumSet => "enum_set",
            Self::Json => "json",
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        Some(match s {
            "bool" => Self::Bool,
            "string" => Self::String,
            "int" => Self::Int,
            "enum_set" => Self::EnumSet,
            "json" => Self::Json,
            _ => return None,
        })
    }

    /// Returns Err with reason if `v` does not match this declared type.
    pub fn validate(&self, v: &Value) -> Result<(), &'static str> {
        match (self, v) {
            (Self::Bool, Value::Bool(_)) => Ok(()),
            (Self::String, Value::String(_)) => Ok(()),
            (Self::Int, Value::Number(n)) if n.is_i64() => Ok(()),
            (Self::EnumSet, Value::Array(arr)) if arr.iter().all(|x| x.is_string()) => Ok(()),
            (Self::Json, _) => Ok(()),
            (Self::Bool, _) => Err("expected boolean"),
            (Self::String, _) => Err("expected string"),
            (Self::Int, _) => Err("expected integer"),
            (Self::EnumSet, _) => Err("expected array of strings"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FeatureDefinition {
    pub slug: String,
    pub value_type: FeatureValueType,
    pub default_value: Value,
    pub description: String,
    pub self_service: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgFeatureOverride {
    pub organisation_id: Uuid,
    pub slug: String,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Uuid,
}

/// Effective value for an (org, slug) pair, with provenance for UI/audit.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EvaluatedFeature {
    pub slug: String,
    pub value: Value,
    pub source: FeatureSource,
    pub value_type: FeatureValueType,
    pub self_service: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureSource {
    Default,
    Override,
}
