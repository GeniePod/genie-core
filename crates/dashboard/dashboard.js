// GeniePod System Dashboard
// Polls /api/status, /api/tegrastats, /api/services every 5 seconds.
// Renders Chart.js time-series for RAM, GPU, and power.

const POLL_MS = 5000;
const MAX_POINTS = 120; // 10 minutes at 5s interval

// --- Chart setup ---
const chartOpts = (label, color, yMax) => ({
  responsive: true,
  maintainAspectRatio: false,
  animation: false,
  plugins: { legend: { display: false } },
  scales: {
    x: { display: false },
    y: {
      min: 0,
      max: yMax,
      ticks: { color: '#556', font: { size: 10 } },
      grid: { color: '#1e2730' },
    },
  },
  elements: { point: { radius: 0 }, line: { borderWidth: 1.5 } },
});

function makeChart(canvasId, label, color, yMax) {
  const ctx = document.getElementById(canvasId);
  if (!ctx) return null;
  return new Chart(ctx, {
    type: 'line',
    data: {
      labels: [],
      datasets: [{
        label,
        data: [],
        borderColor: color,
        backgroundColor: color + '18',
        fill: true,
        tension: 0.3,
      }],
    },
    options: chartOpts(label, color, yMax),
  });
}

let ramChart, gpuChart, powerChart;

function initCharts() {
  ramChart = makeChart('chart-ram', 'RAM Used (MB)', '#00d4ff', 8192);
  gpuChart = makeChart('chart-gpu', 'GPU %', '#7c4dff', 100);
  powerChart = makeChart('chart-power', 'Power (W)', '#ffd740', 20);
}

function pushPoint(chart, label, value) {
  if (!chart) return;
  chart.data.labels.push(label);
  chart.data.datasets[0].data.push(value);
  if (chart.data.labels.length > MAX_POINTS) {
    chart.data.labels.shift();
    chart.data.datasets[0].data.shift();
  }
  chart.update();
}

// --- Mode badge ---
function updateMode(mode) {
  const badge = document.getElementById('mode-badge');
  if (!badge) return;
  const m = (mode || 'unknown').replace(/_/g, '-');
  badge.textContent = m.toUpperCase();
  badge.className = 'mode mode-' + m;
}

// --- Stat values ---
function setText(id, val) {
  const el = document.getElementById(id);
  if (el) el.textContent = val;
}

// --- Fetch helpers ---
async function fetchJson(url) {
  try {
    const r = await fetch(url);
    return await r.json();
  } catch {
    return null;
  }
}

// --- Poll loops ---
async function pollStatus() {
  const data = await fetchJson('/api/status');
  if (!data) return;

  updateMode(data.mode);

  const memAvail = data.mem_available_mb_live ?? data.mem_available_mb ?? 0;
  const memTotal = 7620; // Orin Nano 8GB reports ~7620 MB
  const memUsed = memTotal - memAvail;

  setText('ram-used', memUsed);
  setText('ram-avail', memAvail);
  setText('ram-total', memTotal);

  const now = new Date().toLocaleTimeString([], { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
  pushPoint(ramChart, now, memUsed);
}

async function pollTegrastats() {
  const data = await fetchJson('/api/tegrastats');
  if (!data || !data.length) return;

  // Use the most recent entry.
  const latest = data[0];

  setText('gpu-freq', latest.gpu_pct ?? '--');
  setText('gpu-temp', latest.gpu_c != null ? latest.gpu_c.toFixed(1) : '--');
  setText('cpu-temp', latest.cpu_c != null ? latest.cpu_c.toFixed(1) : '--');

  const powerW = latest.power_mw != null ? (latest.power_mw / 1000).toFixed(1) : '--';
  setText('power', powerW);

  const now = new Date().toLocaleTimeString([], { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
  pushPoint(gpuChart, now, latest.gpu_pct ?? 0);
  pushPoint(powerChart, now, latest.power_mw != null ? latest.power_mw / 1000 : 0);

  // Backfill charts from historical data (only on first load).
  if (ramChart && ramChart.data.labels.length <= 1 && data.length > 1) {
    const history = data.slice().reverse().slice(-MAX_POINTS);
    for (const row of history) {
      const t = new Date(row.ts).toLocaleTimeString([], { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
      pushPoint(ramChart, t, row.ram_used ?? 0);
      pushPoint(gpuChart, t, row.gpu_pct ?? 0);
      pushPoint(powerChart, t, row.power_mw != null ? row.power_mw / 1000 : 0);
    }
  }
}

async function pollServices() {
  const data = await fetchJson('/api/services');
  const tbody = document.getElementById('services-body');
  if (!tbody) return;

  if (!data || !data.length) {
    tbody.innerHTML = '<tr><td colspan="3" style="color:var(--text2)">No data yet</td></tr>';
    return;
  }

  tbody.innerHTML = data.map(s => {
    const dotClass = s.healthy ? 'dot-up' : 'dot-down';
    const status = s.healthy ? 'Healthy' : (s.error || 'Down');
    const statusColor = s.healthy ? 'var(--green)' : 'var(--red)';
    return `<tr>
      <td><span class="dot ${dotClass}"></span>${s.service}</td>
      <td style="color:${statusColor}">${status}</td>
      <td>${s.response_ms}ms</td>
    </tr>`;
  }).join('');
}

// --- Init ---
document.addEventListener('DOMContentLoaded', () => {
  if (typeof Chart !== 'undefined') {
    initCharts();
  } else {
    // Chart.js CDN failed (offline mode) — skip charts.
    console.warn('Chart.js not loaded, charts disabled');
  }

  // Initial polls.
  pollStatus();
  pollTegrastats();
  pollServices();

  // Recurring polls.
  setInterval(pollStatus, POLL_MS);
  setInterval(pollTegrastats, POLL_MS);
  setInterval(pollServices, POLL_MS * 2); // Services change slowly.
});
