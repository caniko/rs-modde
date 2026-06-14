use super::*;

pub(super) fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(super) fn process_snapshot() -> ProcessSnapshot {
    let mut snapshot = ProcessSnapshot::default();
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmRSS:") {
                snapshot.vm_rss_kib = first_number(value);
            } else if let Some(value) = line.strip_prefix("VmSwap:") {
                snapshot.vm_swap_kib = first_number(value);
            } else if let Some(value) = line.strip_prefix("Threads:") {
                snapshot.threads = first_number(value);
            }
        }
    }
    if let Ok(io) = std::fs::read_to_string("/proc/self/io") {
        for line in io.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let parsed = value.trim().parse::<u64>().ok();
            match key {
                "rchar" => snapshot.rchar = parsed,
                "wchar" => snapshot.wchar = parsed,
                "read_bytes" => snapshot.read_bytes = parsed,
                "write_bytes" => snapshot.write_bytes = parsed,
                _ => {}
            }
        }
    }
    snapshot
}

pub fn current_process_snapshot() -> ProcessSnapshot {
    process_snapshot()
}

pub(super) fn cgroup_snapshot() -> Option<CgroupSnapshot> {
    let path = current_cgroup_v2_path()?;
    Some(CgroupSnapshot {
        path: path.display().to_string(),
        memory_current: read_cgroup_u64(&path, "memory.current"),
        memory_high: read_cgroup_limit(&path, "memory.high"),
        memory_max: read_cgroup_limit(&path, "memory.max"),
        memory_swap_current: read_cgroup_u64(&path, "memory.swap.current"),
        memory_swap_max: read_cgroup_limit(&path, "memory.swap.max"),
        memory_events_high: read_memory_event(&path, "high"),
        memory_events_oom: read_memory_event(&path, "oom"),
        memory_events_oom_kill: read_memory_event(&path, "oom_kill"),
    })
}

fn current_cgroup_v2_path() -> Option<PathBuf> {
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    for line in cgroup.lines() {
        let mut fields = line.splitn(3, ':');
        let _hierarchy = fields.next();
        let controllers = fields.next()?;
        let path = fields.next()?;
        if controllers.is_empty() {
            return Some(Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/')));
        }
    }
    None
}

fn read_cgroup_u64(path: &Path, file: &str) -> Option<u64> {
    std::fs::read_to_string(path.join(file))
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn read_cgroup_limit(path: &Path, file: &str) -> Option<u64> {
    let value = std::fs::read_to_string(path.join(file)).ok()?;
    let value = value.trim();
    if value == "max" {
        None
    } else {
        value.parse::<u64>().ok()
    }
}

fn read_memory_event(path: &Path, key: &str) -> Option<u64> {
    let events = std::fs::read_to_string(path.join("memory.events")).ok()?;
    for line in events.lines() {
        let (event, value) = line.split_once(' ')?;
        if event == key {
            return value.parse::<u64>().ok();
        }
    }
    None
}

fn first_number(value: &str) -> Option<u64> {
    value.split_whitespace().next()?.parse::<u64>().ok()
}

pub fn memory_is_saturated(cgroup: Option<&CgroupSnapshot>) -> bool {
    let Some(cgroup) = cgroup else {
        return false;
    };
    let high_saturated = match (cgroup.memory_current, cgroup.memory_high) {
        (Some(current), Some(high)) => current >= high,
        _ => false,
    };
    let swap_saturated = match (cgroup.memory_swap_current, cgroup.memory_swap_max) {
        (Some(current), Some(max)) if max > 0 => current.saturating_mul(100) >= max * 95,
        _ => false,
    };
    high_saturated && swap_saturated
}

pub fn cgroup_memory_pressure_high(threshold: f64) -> bool {
    let Some(snapshot) = cgroup_snapshot() else {
        return false;
    };
    let Some(current) = snapshot.memory_current else {
        return false;
    };
    let limit = snapshot.memory_high.or(snapshot.memory_max);
    let Some(limit) = limit.filter(|limit| *limit > 0) else {
        return false;
    };
    (current as f64 / limit as f64) >= threshold
}
