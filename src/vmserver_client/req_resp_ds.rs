use std::str::FromStr;

use anyhow::bail;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct PingPongPageTrackInitResp {
    gpa1: u64,
    gpa2: u64,
}

#[derive(Serialize, Deserialize)]
pub struct PingPongPageTrackReq {
    access_type: String,
}

#[derive(Serialize, Deserialize)]
pub struct SingleStepVictimStartReq {
    pub victim_program: String,
}

#[derive(Serialize, Deserialize)]
pub struct SingleStepVictimInitReq {
    pub victim_program: String,
}

#[derive(Serialize, Deserialize)]
pub struct SingleStepVictimInitResp {
    /// (Guest) physical address at which the targeted function is mapped
    gpa: u64,
    /// virtual address at which the targeted function is mapped
    vaddr: u64,
    /// all instr offsets of the targeted function (on assembly level), relative to vaddr
    expected_offsets: Vec<u64>,

    /// If true, the following cache attack fields contain valid data
    has_cache_attack_data: bool,
    // Start of cache attack fields
    /// subset of expected_offsets at which relevant memory accesses take place
    offsets_with_mem_access: Vec<u64>,
    /// store for each offsets_with_mem_access which offset in the lookup table is accessed
    mem_access_target_offset: Vec<u64>,
    lookup_table_gpa: u64,
    lookup_table_vaddr: u64,
    /// length of lookup table in bytes
    lookup_table_bytes: u64,
    //end of cache attack fields
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub enum SingleStepTarget {
    NopSlide,
    SimpleCacheTarget,
    SimpleCacheTargetLfence,
    EvalCacheTarget,
    EvalCacheTargetLfence,
}

impl ToString for SingleStepTarget {
    fn to_string(&self) -> String {
        match self {
            Self::NopSlide => String::from("NopSlide"),
            Self::SimpleCacheTarget => String::from("SimpleCacheTarget"),
            Self::SimpleCacheTargetLfence => String::from("SimpleCacheTargetLfence"),
            Self::EvalCacheTarget => String::from("EvalCacheTarget"),
            Self::EvalCacheTargetLfence => String::from("EvalCacheTargetLfence"),
        }
    }
}

impl FromStr for SingleStepTarget {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "NopSlide" => Ok(SingleStepTarget::NopSlide),
            "SimpleCacheTarget" => Ok(SingleStepTarget::SimpleCacheTarget),
            "SimpleCacheTargetLfence" => Ok(SingleStepTarget::SimpleCacheTargetLfence),
            "EvalCacheTarget" => Ok(Self::EvalCacheTarget),
            "EvalCacheTargetLfence" => Ok(Self::EvalCacheTargetLfence),
            _ => bail!("not a valid victim program type"),
        }
    }
}
