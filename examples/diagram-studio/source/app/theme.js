/* Diagram Studio theme control.
   Sets data-theme="light|dark" on <html>; absent means "follow the OS".
   Runs before paint (loaded in <head>, not deferred) to avoid a flash. */
(function () {
  "use strict";

  var STORAGE_KEY = "diagram-studio.theme";
  var MODES = ["light", "auto", "dark"];

  function read() {
    try {
      var stored = window.localStorage.getItem(STORAGE_KEY);
      return MODES.indexOf(stored) === -1 ? "auto" : stored;
    } catch (error) {
      return "auto";
    }
  }

  function apply(mode) {
    var root = document.documentElement;
    if (mode === "auto") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", mode);
  }

  var mode = read();
  apply(mode);

  function setMode(next) {
    mode = next;
    apply(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch (error) {
      /* storage unavailable: the theme still applies for this session */
    }
    sync();
  }

  function sync() {
    MODES.forEach(function (name) {
      var button = document.getElementById("theme-" + name);
      if (button) button.setAttribute("aria-pressed", String(name === mode));
    });
  }

  document.addEventListener("DOMContentLoaded", function () {
    MODES.forEach(function (name) {
      var button = document.getElementById("theme-" + name);
      if (button) {
        button.addEventListener("click", function () {
          setMode(name);
        });
      }
    });
    sync();
  });

  window.diagramStudioTheme = {
    get: function () {
      return mode;
    },
    set: setMode,
  };
})();

