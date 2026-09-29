pub struct ProcStats {
    mem_total_kb: u64,
    mem_available_kb: u64,
    prev_idle: u64,
    prev_total: u64,
    cpu_usage_pct: f32,
}

impl ProcStats {
    pub fn new() -> Self {
        let mut s = Self {
            mem_total_kb: 0,
            mem_available_kb: 0,
            prev_idle: 0,
            prev_total: 0,
            cpu_usage_pct: 0.0,
        };
        s.refresh_memory();
        s.refresh_cpu_usage();
        s
    }

    /// sysinfo `refresh_memory`: MemTotal/MemAvailable from /proc/meminfo.
    pub fn refresh_memory(&mut self) {
        if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
            self.mem_total_kb = meminfo_kb(&text, "MemTotal");
            self.mem_available_kb = meminfo_kb(&text, "MemAvailable");
        }
    }

    /// sysinfo `refresh_cpu_usage`: usage % from /proc/stat jiffies deltas.
    pub fn refresh_cpu_usage(&mut self) {
        let Ok(text) = std::fs::read_to_string("/proc/stat") else {
            return;
        };
        let Some(line) = text.lines().next() else {
            return;
        };
        // "cpu  user nice system idle iowait irq softirq steal ..."
        let fields: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|f| f.parse().ok())
            .collect();
        if fields.len() < 4 {
            return;
        }
        let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
        let total: u64 = fields.iter().sum();
        let d_total = total.saturating_sub(self.prev_total);
        let d_idle = idle.saturating_sub(self.prev_idle);
        if d_total > 0 {
            self.cpu_usage_pct = ((d_total - d_idle) as f32 / d_total as f32) * 100.0;
        }
        self.prev_idle = idle;
        self.prev_total = total;
    }

    pub fn total_memory_bytes(&self) -> u64 {
        self.mem_total_kb * 1024
    }
    pub fn available_memory_bytes(&self) -> u64 {
        self.mem_available_kb * 1024
    }
    pub fn global_cpu_usage(&self) -> f32 {
        self.cpu_usage_pct
    }
}

pub(crate) fn kb_value(field: &str) -> u64 {
    field
        .split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Look up `key:` in meminfo text → its kB value (0 if absent/unparseable).
pub(crate) fn meminfo_kb(text: &str, key: &str) -> u64 {
    for line in text.lines() {
        if let Some(v) = line.strip_prefix(key)
            && let Some(v) = v.strip_prefix(':')
        {
            return kb_value(v);
        }
    }
    0
}
