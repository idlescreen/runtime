// SPDX-License-Identifier: MIT

//! Discovery of heterogeneous CPU topology (Intel P/E cores, ARM big.LITTLE).
//!
//! Multi-tiered detection hierarchy:
//! 1. Intel hybrid core types (`topology/core_type` or `types/intel_atom_*/cpus`)
//! 2. ARM DynamIQ/big.LITTLE capacity (`cpu_capacity`)
//! 3. Maximum clock frequency disparity (`cpufreq/cpuinfo_max_freq`)
//! 4. Single-thread vs SMT siblings (`topology/thread_siblings_list`)
//! 5. Homogeneous CPU fallback

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::tiers::{
    detect_arm_capacity, detect_frequency_disparity, detect_intel_atom, detect_smt_disparity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreDiscoveryTier {
    IntelCoreType,
    ArmCpuCapacity,
    FrequencyDisparity,
    SmtDisparity,
    HomogeneousFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuAffinityPlan {
    pub efficient_cores: Vec<usize>,
    pub performance_cores: Vec<usize>,
    pub total_cores: usize,
    pub tier: CoreDiscoveryTier,
}

impl CpuAffinityPlan {
    pub fn is_heterogeneous(&self) -> bool {
        self.tier != CoreDiscoveryTier::HomogeneousFallback && !self.efficient_cores.is_empty()
    }
}

pub fn parse_cpu_list(content: &str) -> Vec<usize> {
    let mut cpus = BTreeSet::new();
    for part in content.trim().split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start_s, end_s)) = part.split_once('-') {
            if let (Ok(start), Ok(end)) = (
                start_s.trim().parse::<usize>(),
                end_s.trim().parse::<usize>(),
            ) {
                for cpu in start..=end {
                    cpus.insert(cpu);
                }
            }
        } else if let Ok(cpu) = part.parse::<usize>() {
            cpus.insert(cpu);
        }
    }
    cpus.into_iter().collect()
}

pub fn discover_cpu_topology() -> CpuAffinityPlan {
    discover_cpu_topology_at(Path::new("/sys/devices/system/cpu"))
}

pub fn discover_cpu_topology_at(root: &Path) -> CpuAffinityPlan {
    let online_cpus = find_online_cpus(root);
    let total_cores = online_cpus.len();
    if total_cores <= 1 {
        return CpuAffinityPlan {
            efficient_cores: online_cpus.clone(),
            performance_cores: online_cpus,
            total_cores,
            tier: CoreDiscoveryTier::HomogeneousFallback,
        };
    }

    if let Some(plan) = detect_intel_atom(root, &online_cpus) {
        return plan;
    }
    if let Some(plan) = detect_arm_capacity(root, &online_cpus) {
        return plan;
    }
    if let Some(plan) = detect_frequency_disparity(root, &online_cpus) {
        return plan;
    }
    if let Some(plan) = detect_smt_disparity(root, &online_cpus) {
        return plan;
    }

    CpuAffinityPlan {
        efficient_cores: online_cpus.clone(),
        performance_cores: online_cpus,
        total_cores,
        tier: CoreDiscoveryTier::HomogeneousFallback,
    }
}

fn find_online_cpus(root: &Path) -> Vec<usize> {
    if let Ok(online) = fs::read_to_string(root.join("online")) {
        let parsed = parse_cpu_list(&online);
        if !parsed.is_empty() {
            return parsed;
        }
    }
    let mut cpus = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(num_str) = name_str.strip_prefix("cpu")
                && let Ok(cpu_id) = num_str.parse::<usize>()
            {
                cpus.push(cpu_id);
            }
        }
    }
    cpus.sort_unstable();
    if cpus.is_empty() { vec![0] } else { cpus }
}
