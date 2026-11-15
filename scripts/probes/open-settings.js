// Open the in-popup settings panel through the real gear button.
(() => {
  const gear = document.querySelector('button[aria-label="تنظیمات"]');
  if (!gear) return "no gear";
  gear.click();
  return "opened";
})();
