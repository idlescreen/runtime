// SPDX-License-Identifier: MIT

//! Detection tiers for heterogeneous CPU architectures.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::topology::{CoreDiscoveryTier, CpuAffinityPlan, parse_cpu_list};

pub(super) fn detect_intel_atom(root: &Path, online_cpus: &[usize]) -> Option<CpuAffinityPlan> {
    let types_dir = root.join("types");
    if types_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&types_dir)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.contains("atom")
                && let Ok(cpus_str) = fs::read_to_string(entry.path().join("cpus"))
            {
                let e_cores = parse_cpu_list(&cpus_str);
                let e_set: BTreeSet<usize> = e_cores
                    .into_iter()
                    .filter(|c| online_cpus.contains(c))
                    .collect();
                if !e_set.is_empty() && e_set.len() < online_cpus.len() {
                    let p_cores = online_cpus
                        .iter()
                        .copied()
                        .filter(|c| !e_set.contains(c))
                        .collect();
                    return Some(CpuAffinityPlan {
                        efficient_cores: e_set.into_iter().collect(),
                        performance_cores: p_cores,
                        total_cores: online_cpus.len(),
                        tier: CoreDiscoveryTier::IntelCoreType,
                    });
                }
            }
        }
    }

    let mut e_cores = Vec::new();
    let mut p_cores = Vec::new();
    let mut has_core_type = false;
    for &cpu in online_cpus {
        let path = root.join(format!("cpu{cpu}/topology/core_type"));
        if let Ok(val) = fs::read_to_string(path) {
            has_core_type = true;
            let val_lower = val.trim().to_ascii_lowercase();
            if val_lower.contains("atom") || val_lower == "0x20" || val_lower.contains("efficient")
            {
                e_cores.push(cpu);
            } else {
                p_cores.push(cpu);
            }
        }
    }
    if has_core_type && !e_cores.is_empty() && !p_cores.is_empty() {
        Some(CpuAffinityPlan {
            efficient_cores: e_cores,
            performance_cores: p_cores,
            total_cores: online_cpus.len(),
            tier: CoreDiscoveryTier::IntelCoreType,
        })
    } else {
        None
    }
}

pub(super) fn detect_arm_capacity(root: &Path, online_cpus: &[usize]) -> Option<CpuAffinityPlan> {
    let mut capacities: Vec<(usize, u64)> = Vec::new();
    for &cpu in online_cpus {
        let path = root.join(format!("cpu{cpu}/cpu_capacity"));
        if let Ok(content) = fs::read_to_string(path)
            && let Ok(cap) = content.trim().parse::<u64>()
        {
            capacities.push((cpu, cap));
        }
    }
    if capacities.len() != online_cpus.len() {
        return None;
    }
    let min_cap = capacities.iter().map(|(_, c)| *c).min()?;
    let max_cap = capacities.iter().map(|(_, c)| *c).max()?;
    if min_cap == max_cap || max_cap == 0 {
        return None;
    }
    let threshold = min_cap + (max_cap - min_cap) / 3;
    let mut e_cores = Vec::new();
    let mut p_cores = Vec::new();
    for (cpu, cap) in capacities {
        if cap <= threshold {
            e_cores.push(cpu);
        } else {
            p_cores.push(cpu);
        }
    }
    if !e_cores.is_empty() && !p_cores.is_empty() {
        Some(CpuAffinityPlan {
            efficient_cores: e_cores,
            performance_cores: p_cores,
            total_cores: online_cpus.len(),
            tier: CoreDiscoveryTier::ArmCpuCapacity,
        })
    } else {
        None
    }
}

pub(super) fn detect_frequency_disparity(
    root: &Path,
    online_cpus: &[usize],
) -> Option<CpuAffinityPlan> {
    let mut freqs: Vec<(usize, u64)> = Vec::new();
    for &cpu in online_cpus {
        let path = root.join(format!("cpu{cpu}/cpufreq/cpuinfo_max_freq"));
        if let Ok(content) = fs::read_to_string(path)
            && let Ok(freq) = content.trim().parse::<u64>()
        {
            freqs.push((cpu, freq));
        }
    }
    if freqs.len() != online_cpus.len() {
        return None;
    }
    let min_freq = freqs.iter().map(|(_, f)| *f).min()?;
    let max_freq = freqs.iter().map(|(_, f)| *f).max()?;
    if max_freq == 0 || min_freq as f64 / max_freq as f64 > 0.85 {
        return None;
    }
    let threshold = min_freq + (max_freq - min_freq) / 3;
    let mut e_cores = Vec::new();
    let mut p_cores = Vec::new();
    for (cpu, freq) in freqs {
        if freq <= threshold {
            e_cores.push(cpu);
        } else {
            p_cores.push(cpu);
        }
    }
    if !e_cores.is_empty() && !p_cores.is_empty() {
        Some(CpuAffinityPlan {
            efficient_cores: e_cores,
            performance_cores: p_cores,
            total_cores: online_cpus.len(),
            tier: CoreDiscoveryTier::FrequencyDisparity,
        })
    } else {
        None
    }
}

pub(super) fn detect_smt_disparity(root: &Path, online_cpus: &[usize]) -> Option<CpuAffinityPlan> {
    let mut sibling_counts: Vec<(usize, usize)> = Vec::new();
    for &cpu in online_cpus {
        let path = root.join(format!("cpu{cpu}/topology/thread_siblings_list"));
        if let Ok(content) = fs::read_to_string(path) {
            let sibs = parse_cpu_list(&content);
            sibling_counts.push((cpu, sibs.len()));
        }
    }
    if sibling_counts.len() != online_cpus.len() {
        return None;
    }
    let min_sibs = sibling_counts.iter().map(|(_, s)| *s).min()?;
    let max_sibs = sibling_counts.iter().map(|(_, s)| *s).max()?;
    if min_sibs == max_sibs || min_sibs != 1 || max_sibs < 2 {
        return None;
    }
    let mut e_cores = Vec::new();
    let mut p_cores = Vec::new();
    for (cpu, sibs) in sibling_counts {
        if sibs == 1 {
            e_cores.push(cpu);
        } else {
            p_cores.push(cpu);
        }
    }
    if !e_cores.is_empty() && !p_cores.is_empty() {
        Some(CpuAffinityPlan {
            efficient_cores: e_cores,
            performance_cores: p_cores,
            total_cores: online_cpus.len(),
            tier: CoreDiscoveryTier::SmtDisparity,
        })
    } else {
        None
    }
}
