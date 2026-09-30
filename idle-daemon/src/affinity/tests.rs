// SPDX-License-Identifier: MIT

#[cfg(test)]
mod unit_tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::super::sched::apply_affinity;
    use super::super::topology::{
        CoreDiscoveryTier, discover_cpu_topology, discover_cpu_topology_at, parse_cpu_list,
    };

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let n = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "idlescreen_affinity_test_{}_{n}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create_dir_all");
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parse_cpu_list_handles_various_formats() {
        assert_eq!(parse_cpu_list("0-3,5,7-8"), vec![0, 1, 2, 3, 5, 7, 8]);
        assert_eq!(parse_cpu_list("  0 , 2-4  "), vec![0, 2, 3, 4]);
        assert_eq!(parse_cpu_list(""), Vec::<usize>::new());
        assert_eq!(parse_cpu_list("invalid, 1-invalid, 4"), vec![4]);
    }

    #[test]
    fn detect_intel_atom_via_types_dir() {
        let dir = TestDir::new();
        let root = dir.path();
        fs::write(root.join("online"), "0-7\n").expect("write");
        let atom_dir = root.join("types/intel_atom_0");
        fs::create_dir_all(&atom_dir).expect("create_dir");
        fs::write(atom_dir.join("cpulist"), "4-7\n").expect("write");

        let plan = discover_cpu_topology_at(root);
        assert_eq!(plan.tier, CoreDiscoveryTier::IntelCoreType);
        assert_eq!(plan.efficient_cores, vec![4, 5, 6, 7]);
        assert_eq!(plan.performance_cores, vec![0, 1, 2, 3]);
        assert!(plan.is_heterogeneous());
    }

    #[test]
    fn detect_intel_atom_via_types_dir_legacy_cpus() {
        let dir = TestDir::new();
        let root = dir.path();
        fs::write(root.join("online"), "0-7\n").expect("write");
        let atom_dir = root.join("types/intel_atom_0");
        fs::create_dir_all(&atom_dir).expect("create_dir");
        fs::write(atom_dir.join("cpus"), "4-7\n").expect("write");

        let plan = discover_cpu_topology_at(root);
        assert_eq!(plan.tier, CoreDiscoveryTier::IntelCoreType);
        assert_eq!(plan.efficient_cores, vec![4, 5, 6, 7]);
        assert_eq!(plan.performance_cores, vec![0, 1, 2, 3]);
        assert!(plan.is_heterogeneous());
    }

    #[test]
    fn detect_intel_atom_via_core_type() {
        let dir = TestDir::new();
        let root = dir.path();
        fs::write(root.join("online"), "0-3\n").expect("write");
        for cpu in 0..4 {
            let top_dir = root.join(format!("cpu{cpu}/topology"));
            fs::create_dir_all(&top_dir).expect("create_dir");
            let ctype = if cpu < 2 { "Core\n" } else { "Atom\n" };
            fs::write(top_dir.join("core_type"), ctype).expect("write");
        }

        let plan = discover_cpu_topology_at(root);
        assert_eq!(plan.tier, CoreDiscoveryTier::IntelCoreType);
        assert_eq!(plan.efficient_cores, vec![2, 3]);
        assert_eq!(plan.performance_cores, vec![0, 1]);
    }

    #[test]
    fn detect_arm_capacity_disparity() {
        let dir = TestDir::new();
        let root = dir.path();
        fs::write(root.join("online"), "0-3\n").expect("write");
        for cpu in 0..4 {
            let cpu_dir = root.join(format!("cpu{cpu}"));
            fs::create_dir_all(&cpu_dir).expect("create_dir");
            let cap = if cpu < 2 { "446\n" } else { "1024\n" };
            fs::write(cpu_dir.join("cpu_capacity"), cap).expect("write");
        }

        let plan = discover_cpu_topology_at(root);
        assert_eq!(plan.tier, CoreDiscoveryTier::ArmCpuCapacity);
        assert_eq!(plan.efficient_cores, vec![0, 1]);
        assert_eq!(plan.performance_cores, vec![2, 3]);
    }

    #[test]
    fn detect_frequency_disparity() {
        let dir = TestDir::new();
        let root = dir.path();
        fs::write(root.join("online"), "0-3\n").expect("write");
        for cpu in 0..4 {
            let freq_dir = root.join(format!("cpu{cpu}/cpufreq"));
            fs::create_dir_all(&freq_dir).expect("create_dir");
            let freq = if cpu < 2 { "2400000\n" } else { "5000000\n" };
            fs::write(freq_dir.join("cpuinfo_max_freq"), freq).expect("write");
        }

        let plan = discover_cpu_topology_at(root);
        assert_eq!(plan.tier, CoreDiscoveryTier::FrequencyDisparity);
        assert_eq!(plan.efficient_cores, vec![0, 1]);
        assert_eq!(plan.performance_cores, vec![2, 3]);
    }

    #[test]
    fn detect_smt_disparity() {
        let dir = TestDir::new();
        let root = dir.path();
        fs::write(root.join("online"), "0-3\n").expect("write");
        for cpu in 0..4 {
            let top_dir = root.join(format!("cpu{cpu}/topology"));
            fs::create_dir_all(&top_dir).expect("create_dir");
            let sibs = if cpu < 2 {
                format!("{cpu}\n")
            } else {
                format!("{cpu},{}\n", cpu + 4)
            };
            fs::write(top_dir.join("thread_siblings_list"), sibs).expect("write");
        }

        let plan = discover_cpu_topology_at(root);
        assert_eq!(plan.tier, CoreDiscoveryTier::SmtDisparity);
        assert_eq!(plan.efficient_cores, vec![0, 1]);
        assert_eq!(plan.performance_cores, vec![2, 3]);
    }

    #[test]
    fn homogeneous_cpu_fallback() {
        let dir = TestDir::new();
        let root = dir.path();
        fs::write(root.join("online"), "0-3\n").expect("write");
        for cpu in 0..4 {
            let freq_dir = root.join(format!("cpu{cpu}/cpufreq"));
            fs::create_dir_all(&freq_dir).expect("create_dir");
            fs::write(freq_dir.join("cpuinfo_max_freq"), "3200000\n").expect("write");
        }

        let plan = discover_cpu_topology_at(root);
        assert_eq!(plan.tier, CoreDiscoveryTier::HomogeneousFallback);
        assert_eq!(plan.total_cores, 4);
        assert!(!plan.is_heterogeneous());
    }

    #[test]
    fn real_system_topology_succeeds() {
        let plan = discover_cpu_topology();
        assert!(plan.total_cores >= 1);
        assert!(!plan.efficient_cores.is_empty());
    }

    #[test]
    fn apply_affinity_empty_fails() {
        let res = apply_affinity(&[]);
        assert!(res.is_err());
    }

    #[test]
    fn apply_affinity_valid_succeeds() {
        #[cfg(target_os = "linux")]
        let orig = unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            if libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut set) == 0 {
                Some(set)
            } else {
                None
            }
        };

        let plan = discover_cpu_topology();
        let res = apply_affinity(&plan.efficient_cores);
        assert!(res.is_ok());

        #[cfg(target_os = "linux")]
        if let Some(set) = orig {
            unsafe {
                libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
            }
        }
    }
}
