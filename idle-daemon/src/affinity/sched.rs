// SPDX-License-Identifier: MIT

//! Process CPU core affinity scheduling via `sched_setaffinity`.
//!
//! Restricts child plugin execution to energy-efficient cores (E-cores) on
//! heterogeneous architectures, while gracefully maintaining all cores on
//! homogeneous systems.

use std::io::{Error, ErrorKind};

use super::topology::{CpuAffinityPlan, discover_cpu_topology};

/// Initialize CPU affinity for the daemon process.
///
/// Discovers topology and, if heterogeneous cores are detected, pins the process
/// to the energy-efficient core set. All child processes spawned via `Command::new`
/// naturally inherit this affinity mask.
pub fn init_process_affinity() -> CpuAffinityPlan {
    let plan = discover_cpu_topology();
    if plan.is_heterogeneous() {
        idle_log::info!(
            tier = ?plan.tier,
            e_cores = ?plan.efficient_cores,
            p_cores = ?plan.performance_cores,
            total = plan.total_cores,
            "heterogeneous CPU detected: pinning to efficient cores"
        );
        if let Err(err) = apply_affinity(&plan.efficient_cores) {
            idle_log::warn!(%err, "failed to apply CPU affinity mask; running on unpinned cores");
        }
    } else {
        idle_log::info!(
            total = plan.total_cores,
            "homogeneous CPU detected: keeping default core allocation"
        );
    }
    plan
}

/// Apply a core set mask to the calling process using `sched_setaffinity`.
pub fn apply_affinity(cores: &[usize]) -> Result<(), Error> {
    if cores.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "core list must not be empty",
        ));
    }

    #[cfg(target_os = "linux")]
    {
        // SAFETY: `set` is zeroed before any bits are set; `CPU_SET` operates
        // within the `cpu_set_t` bit range, and `size_of::<cpu_set_t>()` accurately
        // describes the buffer passed to `sched_setaffinity`. pid 0 targets current process.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_ZERO(&mut set);
            for &core in cores {
                if core < (libc::CPU_SETSIZE as usize) {
                    libc::CPU_SET(core, &mut set);
                }
            }
            let res = libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
            if res != 0 {
                return Err(Error::last_os_error());
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = cores;
        Ok(())
    }
}
