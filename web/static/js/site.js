"use strict";
document.addEventListener("click", async (event) => {
  const button = event.target.closest("[data-copy]");
  if (!button) return;
  const content = document.getElementById(button.dataset.copy);
  const status = document.getElementById("copy-status");
  try {
    await navigator.clipboard.writeText(content.textContent);
    status.textContent = "copied to clipboard";
  } catch {
    status.textContent = "select and copy the command manually";
  }
  window.setTimeout(() => { status.textContent = ""; }, 4000);
});
