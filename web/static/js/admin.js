'use strict';
const monitor = document.querySelector('#job-monitor');
const state = document.querySelector('#job-state');
if (monitor && state && state.textContent === 'running') {
  const poll = async () => {
    try {
      const response = await fetch(`/api/v1/jobs/${encodeURIComponent(monitor.dataset.job)}`, {cache: 'no-store'});
      if (!response.ok) { monitor.textContent = 'Status unavailable. Refresh the page to continue.'; return; }
      const job = await response.json();
      state.textContent = job.state;
      monitor.textContent = job.message;
      if (job.state === 'running') window.setTimeout(poll, 2000);
    } catch { monitor.textContent = 'Connection interrupted. Refresh the page to continue.'; }
  };
  window.setTimeout(poll, 1000);
}
