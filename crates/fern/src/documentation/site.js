// Fern documentation site behavior: theme toggle, mobile navigation and local search.
// This script only reads the bundled index and page text; it never fetches or evaluates data.
(function () {
  "use strict";
  var root = document.documentElement;
  var storageKey = "fern-docs-theme";

  function storedTheme() {
    try {
      var value = localStorage.getItem(storageKey);
      return value === "dark" || value === "light" ? value : null;
    } catch (error) {
      return null;
    }
  }
  var initial = storedTheme();
  if (initial) {
    root.setAttribute("data-theme", initial);
  }
  var theme = document.getElementById("theme");
  if (theme) {
    theme.addEventListener("click", function () {
      var current = root.getAttribute("data-theme");
      var dark = current === "dark" ||
        (!current && window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches);
      var next = dark ? "light" : "dark";
      root.setAttribute("data-theme", next);
      try {
        localStorage.setItem(storageKey, next);
      } catch (error) {
        // Storage may be unavailable; the toggle still applies to this page.
      }
    });
  }

  var menu = document.getElementById("menu");
  var sidebar = document.getElementById("sidebar");
  if (menu && sidebar) {
    menu.addEventListener("click", function () {
      var open = sidebar.classList.toggle("open");
      menu.setAttribute("aria-expanded", open ? "true" : "false");
    });
  }

  var input = document.getElementById("search");
  var results = document.getElementById("search-results");
  var lists = document.getElementById("nav-lists");
  var index = Array.isArray(window.FERN_SEARCH_INDEX) ? window.FERN_SEARCH_INDEX : [];
  var maxResults = 50;

  function score(entry, terms) {
    var name = String(entry.n || "").toLowerCase();
    var context = String(entry.m || "").toLowerCase();
    var description = String(entry.d || "").toLowerCase();
    var total = 0;
    for (var i = 0; i < terms.length; i += 1) {
      var term = terms[i];
      var part = 0;
      if (name === term) {
        part = 100;
      } else if (name.indexOf(term) === 0) {
        part = 60;
      } else if (name.indexOf(term) !== -1) {
        part = 35;
      } else if (context.indexOf(term) !== -1) {
        part = 20;
      } else if (description.indexOf(term) !== -1) {
        part = 10;
      }
      if (part === 0) {
        return 0;
      }
      total += part;
    }
    if (entry.t === "module" || entry.t === "guide") {
      total += 5;
    }
    return total;
  }

  function clear() {
    if (!results || !lists) {
      return;
    }
    while (results.firstChild) {
      results.removeChild(results.firstChild);
    }
    results.hidden = true;
    lists.hidden = false;
  }

  function render(query) {
    if (!results || !lists) {
      return;
    }
    var terms = query.toLowerCase().split(/\s+/).filter(function (term) {
      return term.length > 0;
    });
    if (terms.length === 0) {
      clear();
      return;
    }
    var matches = [];
    for (var i = 0; i < index.length; i += 1) {
      var value = score(index[i], terms);
      if (value > 0) {
        matches.push({ entry: index[i], score: value });
      }
    }
    matches.sort(function (a, b) {
      if (b.score !== a.score) {
        return b.score - a.score;
      }
      return String(a.entry.n).localeCompare(String(b.entry.n));
    });
    while (results.firstChild) {
      results.removeChild(results.firstChild);
    }
    if (matches.length === 0) {
      var empty = document.createElement("li");
      empty.className = "empty";
      empty.textContent = "No results";
      results.appendChild(empty);
    }
    for (var j = 0; j < matches.length && j < maxResults; j += 1) {
      var entry = matches[j].entry;
      var item = document.createElement("li");
      var link = document.createElement("a");
      link.href = String(entry.u || "#");
      var kind = document.createElement("span");
      kind.className = "result-kind";
      kind.textContent = String(entry.t || "");
      var name = document.createElement("strong");
      name.textContent = String(entry.n || "");
      var context = document.createElement("small");
      context.textContent = String(entry.m || "") + (entry.d ? " — " + String(entry.d) : "");
      link.appendChild(kind);
      link.appendChild(name);
      link.appendChild(context);
      item.appendChild(link);
      results.appendChild(item);
    }
    results.hidden = false;
    lists.hidden = true;
  }

  if (input) {
    input.addEventListener("input", function () {
      render(input.value);
    });
    input.addEventListener("keydown", function (event) {
      if (event.key === "Escape") {
        input.value = "";
        clear();
        input.blur();
      } else if (event.key === "Enter" && results && !results.hidden) {
        var first = results.querySelector("a");
        if (first) {
          event.preventDefault();
          first.click();
        }
      }
    });
    document.addEventListener("keydown", function (event) {
      var target = event.target;
      var editing = target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable);
      if (event.key === "/" && !editing && !event.ctrlKey && !event.metaKey && !event.altKey) {
        event.preventDefault();
        if (sidebar) {
          sidebar.classList.add("open");
        }
        input.focus();
        input.select();
      }
    });
  }
})();
