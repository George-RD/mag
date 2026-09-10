/* Shared playback for recorded walkthroughs. No MAG runtime or network calls. */
(function (global) {
  "use strict";
  function reduced() {
    return !!(global.matchMedia && global.matchMedia("(prefers-reduced-motion: reduce)").matches);
  }
  function pad(number) { return (number < 10 ? "0" : "") + number; }
  function escape(text) {
    return String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
  function mount(states, render) {
    var current = 0, timer = null, resumeOnShow = false;
    var el = {};
    ["steps", "scrubber", "scrubval", "play", "prev", "next", "first", "last"].forEach(function (key) {
      el[key] = document.getElementById(key);
    });
    var storageKey = "mag-demo-play:" + global.location.pathname.split("/").pop();
    function save(value) {
      try { global.localStorage.setItem(storageKey, value ? "1" : "0"); } catch (_) { /* storage is optional */ }
    }
    function saved() {
      try { return global.localStorage.getItem(storageKey) === "1"; } catch (_) { return false; }
    }
    function stopTimer() {
      if (timer !== null) { global.clearInterval(timer); timer = null; }
      el.play.setAttribute("aria-pressed", "false");
      el.play.textContent = "\u25b6 Play";
    }
    function stop() {
      stopTimer();
      save(false);
    }
    function go(index, keepHash) {
      current = Math.max(0, Math.min(states.length - 1, index));
      render(current);
      Array.prototype.forEach.call(el.steps.children, function (button, i) {
        if (i === current) { button.setAttribute("aria-current", "step"); }
        else { button.removeAttribute("aria-current"); }
      });
      el.scrubber.value = String(current + 1);
      el.scrubval.textContent = pad(current + 1) + " / " + pad(states.length);
      el.prev.disabled = el.first.disabled = current === 0;
      el.next.disabled = el.last.disabled = current === states.length - 1;
      if (!keepHash) {
        try { global.history.replaceState(null, "", "#" + states[current].id); }
        catch (_) { global.location.hash = states[current].id; }
      }
      if (current === states.length - 1 && timer !== null) { stop(); }
    }
    function start() {
      if (timer !== null || current === states.length - 1) { return; }
      el.play.setAttribute("aria-pressed", "true");
      el.play.textContent = "\u275a\u275a Pause";
      save(true);
      timer = global.setInterval(function () { go(current + 1); }, 3600);
    }
    function choose(index) { stop(); go(index); }
    function hashIndex() {
      var hash = global.location.hash.slice(1);
      return states.findIndex(function (state) { return state.id === hash; });
    }
    states.forEach(function (state, index) {
      var button = document.createElement("button");
      button.type = "button";
      button.className = "step";
      button.textContent = pad(index + 1) + " " + state.name;
      button.addEventListener("click", function () { choose(index); });
      el.steps.appendChild(button);
    });
    el.scrubber.max = String(states.length);
    el.play.addEventListener("click", function () { if (timer !== null) { stop(); } else { start(); } });
    el.next.addEventListener("click", function () { choose(current + 1); });
    el.prev.addEventListener("click", function () { choose(current - 1); });
    el.first.addEventListener("click", function () { choose(0); });
    el.last.addEventListener("click", function () { choose(states.length - 1); });
    el.scrubber.addEventListener("input", function () {
      var value = Number(el.scrubber.value);
      if (Number.isInteger(value)) { choose(value - 1); }
    });
    global.addEventListener("hashchange", function () {
      var index = hashIndex();
      if (index >= 0 && index !== current) { stop(); go(index, true); }
    });
    global.addEventListener("pagehide", function () {
      resumeOnShow = timer !== null;
      stopTimer(); // Preserve the explicit preference across reloads.
    });
    global.addEventListener("pageshow", function (event) {
      if (event.persisted) {
        if (resumeOnShow && !reduced()) { start(); }
        resumeOnShow = false;
      }
    });
    var index = hashIndex();
    go(index >= 0 ? index : 0, index >= 0);
    if (!reduced() && saved()) { start(); }
  }
  global.MagDemoPlayer = { mount: mount, reduced: reduced, pad: pad, escape: escape };
})(window);
