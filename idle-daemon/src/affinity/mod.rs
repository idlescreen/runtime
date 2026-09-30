// SPDX-License-Identifier: MIT

//! Heterogeneous CPU core pinning and energy-efficient E-core scheduling.
//!
//! On hybrid architectures (e.g. Intel Alder/Raptor Lake, ARM big.LITTLE),
//! screensaver rendering workloads are pinned to high-efficiency E-cores
//! to preserve thermal and battery headroom.

pub mod sched;
pub(crate) mod tiers;
pub mod topology;

#[cfg(test)]
mod tests;

pub use sched::{apply_affinity, init_process_affinity};
pub use topology::{
    CoreDiscoveryTier, CpuAffinityPlan, discover_cpu_topology, discover_cpu_topology_at,
};
