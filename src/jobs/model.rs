use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Pending,
    Running,
    Done,
    Dead,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Pending => "pending",
            JobState::Running => "running",
            JobState::Done => "done",
            JobState::Dead => "dead",
        }
    }
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for JobState {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(JobState::Pending),
            "running" => Ok(JobState::Running),
            "done" => Ok(JobState::Done),
            "dead" => Ok(JobState::Dead),
            other => anyhow::bail!("unknown job state: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub kind: String,
    pub payload: serde_json::Value,
    pub state: JobState,
    pub attempts: i32,
    pub max_attempts: i32,
    pub run_at: DateTime<Utc>,
    pub locked_until: Option<DateTime<Utc>>,
    pub locked_by: Option<String>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct EnqueueRequest {
    pub kind: String,
    pub payload: serde_json::Value,
    pub max_attempts: i32,
    pub run_at: DateTime<Utc>,
}

impl EnqueueRequest {
    pub fn now(kind: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            payload,
            max_attempts: 5,
            run_at: Utc::now(),
        }
    }

    pub fn with_max_attempts(mut self, n: i32) -> Self {
        self.max_attempts = n;
        self
    }

    pub fn with_run_at(mut self, when: DateTime<Utc>) -> Self {
        self.run_at = when;
        self
    }
}
