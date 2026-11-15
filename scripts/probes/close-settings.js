// Close the in-popup settings panel if it is open.
(() => {
  const overlay = document.querySelector('.settings-overlay');
  const closeButton = overlay?.querySelector('button[aria-label="بستن تنظیمات"]');
  if (overlay && overlay.getAttribute('aria-hidden') === 'false' && closeButton) {
    closeButton.click();
    return "closed";
  }
  return "already closed";
})();
