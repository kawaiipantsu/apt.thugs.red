'use strict';
const monitor = document.querySelector('#job-monitor');
const state = document.querySelector('#job-state');
if (monitor && state && state.textContent === 'running') {
  const poll = async () => {
    try {
      const response = await fetch(`/admin/api/v1/jobs/${encodeURIComponent(monitor.dataset.job)}`, {cache: 'no-store'});
      if (!response.ok) { monitor.textContent = 'Status unavailable. Refresh the page to continue.'; return; }
      const job = await response.json();
      state.textContent = job.state;
      monitor.textContent = job.message;
      if (job.state === 'running') window.setTimeout(poll, 2000);
    } catch { monitor.textContent = 'Connection interrupted. Refresh the page to continue.'; }
  };
  window.setTimeout(poll, 1000);
}

for (const chart of document.querySelectorAll('.interactive-chart')) {
  const svg = chart.querySelector('svg');
  const points = [...chart.querySelectorAll('.chart-point')];
  const readout = chart.querySelector('.chart-readout');
  let selected = points.length - 1;
  const select = index => {
    selected = Math.max(0, Math.min(points.length - 1, index));
    points.forEach((point, i) => point.classList.toggle('is-selected', i === selected));
    readout.textContent = points[selected]?.dataset.label || 'No observations';
  };
  chart.addEventListener('keydown', event => {
    const direction = {ArrowLeft: -1, ArrowRight: 1};
    if (event.key in direction) { event.preventDefault(); select(selected + direction[event.key]); }
    else if (event.key === 'Home' || event.key === 'End') { event.preventDefault(); select(event.key === 'Home' ? 0 : points.length - 1); }
  });
  svg.addEventListener('pointermove', event => {
    const rectangle = svg.getBoundingClientRect();
    const x = (event.clientX - rectangle.left) / rectangle.width * 1000;
    select(Math.round((x - 20) / 960 * (points.length - 1)));
  });
  chart.addEventListener('focus', () => select(selected));
}
