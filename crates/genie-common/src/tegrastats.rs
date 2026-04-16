use anyhow::Result;
use serde::Serialize;

/// Parsed snapshot from a single `tegrastats` output line.
///
/// `tegrastats` is NVIDIA's built-in Jetson monitoring tool.
/// Format (Orin Nano): `RAM 2345/7620MB (lfb 234x4MB) SWAP 0/0MB ... GR3D_FREQ 50% ...`
#[derive(Debug, Clone, Serialize)]
pub struct TegraSnapshot {
    pub timestamp_ms: u64,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub swap_used_mb: u64,
    pub swap_total_mb: u64,
    pub gpu_freq_pct: u8,
    pub cpu_loads: Vec<CpuCore>,
    pub gpu_temp_c: Option<f32>,
    pub cpu_temp_c: Option<f32>,
    pub power_mw: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CpuCore {
    pub id: u8,
    pub load_pct: u8,
    pub freq_mhz: u32,
}

impl TegraSnapshot {
    /// Available RAM in MB (total - used).
    pub fn ram_available_mb(&self) -> u64 {
        self.ram_total_mb.saturating_sub(self.ram_used_mb)
    }
}

/// Parse a single line of `tegrastats` output.
///
/// Example line:
/// ```text
/// RAM 2345/7620MB (lfb 234x4MB) SWAP 0/3810MB (cached 0MB) CPU [20%@1510,15%@1510,10%@1510,8%@1510,off,off] ... GR3D_FREQ 50% ... gpu@42C cpu@38.5C ... VDD_IN 4500mW/4500mW
/// ```
pub fn parse_line(line: &str, timestamp_ms: u64) -> Result<TegraSnapshot> {
    let ram = parse_ram(line)?;
    let swap = parse_swap(line);
    let gpu_freq = parse_gpu_freq(line);
    let cpus = parse_cpus(line);
    let gpu_temp = parse_temp(line, "gpu@");
    let cpu_temp = parse_temp(line, "cpu@");
    let power = parse_power(line);

    Ok(TegraSnapshot {
        timestamp_ms,
        ram_used_mb: ram.0,
        ram_total_mb: ram.1,
        swap_used_mb: swap.0,
        swap_total_mb: swap.1,
        gpu_freq_pct: gpu_freq,
        cpu_loads: cpus,
        gpu_temp_c: gpu_temp,
        cpu_temp_c: cpu_temp,
        power_mw: power,
    })
}

fn parse_ram(line: &str) -> Result<(u64, u64)> {
    // "RAM 2345/7620MB"
    let ram_pos = line
        .find("RAM ")
        .ok_or_else(|| anyhow::anyhow!("no RAM field"))?;
    let rest = &line[ram_pos + 4..];
    let mb_pos = rest.find("MB").unwrap_or(rest.len());
    let fraction = &rest[..mb_pos];
    let (used, total) = fraction
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("no / in RAM field"))?;
    Ok((used.trim().parse()?, total.trim().parse()?))
}

fn parse_swap(line: &str) -> (u64, u64) {
    // "SWAP 0/3810MB"
    if let Some(pos) = line.find("SWAP ") {
        let rest = &line[pos + 5..];
        if let Some(mb_pos) = rest.find("MB") {
            let fraction = &rest[..mb_pos];
            if let Some((used, total)) = fraction.split_once('/')
                && let (Ok(u), Ok(t)) = (used.trim().parse(), total.trim().parse())
            {
                return (u, t);
            }
        }
    }
    (0, 0)
}

fn parse_gpu_freq(line: &str) -> u8 {
    // "GR3D_FREQ 50%"
    if let Some(pos) = line.find("GR3D_FREQ ") {
        let rest = &line[pos + 10..];
        if let Some(pct_pos) = rest.find('%')
            && let Ok(v) = rest[..pct_pos].trim().parse()
        {
            return v;
        }
    }
    0
}

fn parse_cpus(line: &str) -> Vec<CpuCore> {
    // "CPU [20%@1510,15%@1510,off,off]"
    let mut cores = Vec::new();
    if let Some(start) = line.find("CPU [") {
        let rest = &line[start + 5..];
        if let Some(end) = rest.find(']') {
            for (i, part) in rest[..end].split(',').enumerate() {
                let part = part.trim();
                if part == "off" {
                    continue;
                }
                if let Some((load_str, freq_str)) = part.split_once('@') {
                    let load = load_str.trim_end_matches('%').parse().unwrap_or(0);
                    let freq = freq_str.parse().unwrap_or(0);
                    cores.push(CpuCore {
                        id: i as u8,
                        load_pct: load,
                        freq_mhz: freq,
                    });
                }
            }
        }
    }
    cores
}

fn parse_temp(line: &str, prefix: &str) -> Option<f32> {
    // "gpu@42C" or "cpu@38.5C"
    if let Some(pos) = line.find(prefix) {
        let rest = &line[pos + prefix.len()..];
        let end = rest.find('C').unwrap_or(rest.len());
        return rest[..end].trim().parse().ok();
    }
    None
}

fn parse_power(line: &str) -> Option<u32> {
    // "VDD_IN 4500mW/4500mW" — we want the instantaneous (first) value
    if let Some(pos) = line.find("VDD_IN ") {
        let rest = &line[pos + 7..];
        if let Some(mw_pos) = rest.find("mW") {
            return rest[..mw_pos].trim().parse().ok();
        }
    }
    None
}

/// Read `/proc/meminfo` and return MemAvailable in MB.
pub fn mem_available_mb() -> Result<u64> {
    let contents = std::fs::read_to_string("/proc/meminfo")?;
    for line in contents.lines() {
        if let Some(rest) = line.strip_prefix("MemAvailable:") {
            let kb_str = rest.trim().trim_end_matches(" kB").trim();
            let kb: u64 = kb_str.parse()?;
            return Ok(kb / 1024);
        }
    }
    anyhow::bail!("MemAvailable not found in /proc/meminfo")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "RAM 2345/7620MB (lfb 234x4MB) SWAP 0/3810MB (cached 0MB) CPU [20%@1510,15%@1510,10%@1510,8%@1510,off,off] EMC_FREQ 0% GR3D_FREQ 50% VIC_FREQ 0% APE 174 gpu@42C cpu@38.5C iwlwifi@37C CV0@-256C CV1@-256C CV2@-256C SOC2@38.5C SOC0@40C SOC1@37.5C tj@42C VDD_IN 4500mW/4500mW VDD_CPU_GPU_CV 799mW/799mW VDD_SOC 1598mW/1598mW";

    #[test]
    fn parse_ram_values() {
        let snap = parse_line(SAMPLE, 0).unwrap();
        assert_eq!(snap.ram_used_mb, 2345);
        assert_eq!(snap.ram_total_mb, 7620);
        assert_eq!(snap.ram_available_mb(), 7620 - 2345);
    }

    #[test]
    fn parse_swap_values() {
        let snap = parse_line(SAMPLE, 0).unwrap();
        assert_eq!(snap.swap_used_mb, 0);
        assert_eq!(snap.swap_total_mb, 3810);
    }

    #[test]
    fn parse_gpu_freq_value() {
        let snap = parse_line(SAMPLE, 0).unwrap();
        assert_eq!(snap.gpu_freq_pct, 50);
    }

    #[test]
    fn parse_cpu_cores() {
        let snap = parse_line(SAMPLE, 0).unwrap();
        assert_eq!(snap.cpu_loads.len(), 4); // 4 active, 2 off
        assert_eq!(snap.cpu_loads[0].load_pct, 20);
        assert_eq!(snap.cpu_loads[0].freq_mhz, 1510);
    }

    #[test]
    fn parse_temperatures() {
        let snap = parse_line(SAMPLE, 0).unwrap();
        assert_eq!(snap.gpu_temp_c, Some(42.0));
        assert_eq!(snap.cpu_temp_c, Some(38.5));
    }

    #[test]
    fn parse_power_draw() {
        let snap = parse_line(SAMPLE, 0).unwrap();
        assert_eq!(snap.power_mw, Some(4500));
    }
}
