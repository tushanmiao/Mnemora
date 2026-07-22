use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryLayer {
    #[serde(rename = "l1")]
    L1,
    #[serde(rename = "l2")]
    L2,
}

impl MemoryLayer {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::L1 => "L1.md",
            Self::L2 => "L2.md",
        }
    }

    pub fn max_bytes(self) -> usize {
        match self {
            Self::L1 => 5_000,
            Self::L2 => 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryOperation {
    #[serde(rename = "append")]
    Append,
    #[serde(rename = "replace")]
    Replace,
    #[serde(rename = "remove")]
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryModification {
    pub layer: MemoryLayer,
    pub operation: MemoryOperation,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub inject_l1: bool,
    #[serde(default = "default_true")]
    pub allow_model_read: bool,
    #[serde(default)]
    pub allow_model_write: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            inject_l1: true,
            allow_model_read: true,
            allow_model_write: false,
        }
    }
}
