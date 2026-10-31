// The theme script of the shared design system (ADR-0093), at a stable address:
// `/shared/theme.js`. It stamps the Theme (`data-theme`: acogni | classic) and
// the mode (`data-mode`: dark | light) this browser chose on <html>, before the
// first paint, so a page never paints one look and flips to the other. The
// interface loads it from its index.html; a page an extension serves outside
// the interface loads it the same way, next to `/shared/tokens.css`:
//
//   <script src="/shared/theme.js"></script>
//
// A classic script, in the <head>, without `defer` or `async`. It applies the
// rule of `src/lib/theme.ts` (`src/noFlashTheme.test.ts` holds them together):
// Acogni and dark by default, a value stored before 2.68 (`light`, `dark` or
// `system` under `pdo.ui.theme`) migrated once, any other unknown value
// rewritten as the default. It repaints the page when another page of the
// daemon changes the preferences. Dependency-free and
// total: a throwing store must not blank the page.
(function () {
  function stamp() {
    var theme = null;
    var mode = null;
    try {
      theme = localStorage.getItem("pdo.ui.theme");
      mode = localStorage.getItem("pdo.ui.mode");
    } catch (e) {}
    if (theme === "light" || theme === "dark" || theme === "system") {
      var osDark = true;
      try {
        osDark = matchMedia("(prefers-color-scheme: dark)").matches;
      } catch (e) {}
      mode = theme === "system" ? (osDark ? "dark" : "light") : theme;
      theme = "acogni";
      try {
        localStorage.setItem("pdo.ui.theme", theme);
        localStorage.setItem("pdo.ui.mode", mode);
      } catch (e) {}
    }
    if (theme !== null && theme !== "acogni" && theme !== "classic") {
      theme = "acogni";
      try {
        localStorage.setItem("pdo.ui.theme", theme);
      } catch (e) {}
    }
    if (mode !== null && mode !== "dark" && mode !== "light") {
      mode = "dark";
      try {
        localStorage.setItem("pdo.ui.mode", mode);
      } catch (e) {}
    }
    if (theme === null) theme = "acogni";
    if (mode === null) mode = "dark";
    var root = document.documentElement;
    root.setAttribute("data-theme", theme);
    root.setAttribute("data-mode", mode);
    root.style.colorScheme = mode;
  }
  try {
    stamp();
  } catch (e) {}
  try {
    window.addEventListener("storage", function (event) {
      if (event.key === "pdo.ui.theme" || event.key === "pdo.ui.mode" || event.key === null) {
        try {
          stamp();
        } catch (e) {}
      }
    });
  } catch (e) {}
})();
