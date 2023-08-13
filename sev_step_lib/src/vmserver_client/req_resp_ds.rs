use std::str::FromStr;

use anyhow::bail;

use serde::{Deserialize, Serialize};

use crate::types::kvm_page_track_mode;

#[derive(Deserialize, Serialize, Debug)]
pub struct PingPongPageTrackInitResp {
    pub gpa1: u64,
    pub gpa2: u64,
    pub iterations: u64,
}

///Selects which kind of access a PingPongerJob should perform to its two pages
#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum AccessType {
    READ,
    WRITE,
    EXEC,
}

impl ToString for AccessType {
    fn to_string(&self) -> String {
        match self {
            AccessType::READ => String::from("READ"),
            AccessType::WRITE => String::from("WRITE"),
            AccessType::EXEC => String::from("EXEC"),
        }
    }
}

impl FromStr for AccessType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "READ" => Ok(AccessType::READ),
            "WRITE" => Ok(AccessType::WRITE),
            "EXEC" => Ok(AccessType::EXEC),
            _ => bail!("not a valid access type"),
        }
    }
}

impl TryFrom<kvm_page_track_mode> for AccessType {
    type Error = anyhow::Error;

    fn try_from(value: kvm_page_track_mode) -> Result<Self, Self::Error> {
        match value {
            kvm_page_track_mode::KVM_PAGE_TRACK_WRITE => Ok(AccessType::WRITE),
            kvm_page_track_mode::KVM_PAGE_TRACK_ACCESS => Ok(AccessType::READ),
            kvm_page_track_mode::KVM_PAGE_TRACK_EXEC => Ok(AccessType::EXEC),
            _ => bail!("cannot be converted to AccessType"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct PingPongPageTrackReq {
    pub access_type: AccessType,
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
    pub gpa: u64,
    /// virtual address at which the targeted function is mapped
    pub vaddr: u64,
    /// all instr offsets of the targeted function (on assembly level), relative to vaddr
    pub expected_offsets: Vec<u64>,

    /// If true, the following cache attack fields contain valid data
    pub has_cache_attack_data: bool,
    // Start of cache attack fields
    /// subset of expected_offsets at which relevant memory accesses take place
    pub offsets_with_mem_access: Vec<u64>,
    /// store for each offsets_with_mem_access which offset in the lookup table is accessed
    pub mem_access_target_offset: Vec<u64>,
    pub lookup_table_gpa: u64,
    pub lookup_table_vaddr: u64,
    /// length of lookup table in bytes
    pub lookup_table_bytes: u64,
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
