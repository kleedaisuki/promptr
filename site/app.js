(() => {
  "use strict";

  const root = document.documentElement;
  const themeSelect = document.querySelector("#theme");
  const themeColor = document.querySelector('meta[name="theme-color"]');
  const media = matchMedia("(prefers-color-scheme: dark)");

  function savedTheme() {
    let value;
    try {
      value = localStorage.getItem("promptr-theme");
    } catch {
      return "auto";
    }
    return value === "light" || value === "dark" ? value : "auto";
  }

  function applyTheme(value) {
    root.dataset.moeTheme = value;
    try {
      if (value === "auto") localStorage.removeItem("promptr-theme");
      else localStorage.setItem("promptr-theme", value);
    } catch {}

    const dark = value === "dark" || (value === "auto" && media.matches);
    themeColor?.setAttribute("content", dark ? "#21130f" : "#fff6ea");
  }

  if (themeSelect instanceof HTMLSelectElement) {
    themeSelect.value = savedTheme();
    applyTheme(themeSelect.value);
    themeSelect.addEventListener("change", () => applyTheme(themeSelect.value));
  }
  media.addEventListener?.("change", () => {
    if (savedTheme() === "auto") applyTheme("auto");
  });

  const status = document.querySelector(".copy-status");
  let statusTimer;
  function announce(message) {
    if (!(status instanceof HTMLElement)) return;
    status.textContent = message;
    status.dataset.visible = "true";
    clearTimeout(statusTimer);
    statusTimer = setTimeout(() => delete status.dataset.visible, 1800);
  }

  document.querySelectorAll("[data-copy]").forEach((button) => {
    button.addEventListener("click", async () => {
      const selector = button.getAttribute("data-copy");
      const source = selector && document.querySelector(selector);
      if (!source) return;
      try {
        await navigator.clipboard.writeText(source.textContent ?? "");
        announce("已复制到剪贴板");
      } catch {
        announce("无法自动复制，请手动选择文本");
      }
    });
  });
})();
