// Draws the map in the page, in a <crawl-map> element, from the data the
// server sends: the server decides what each player sees, and this only
// draws it. `mapHtml` makes the same HTML the server's game template draws
// the map as, square for square, so the page's style applies unchanged; a
// tile renderer can replace it without the server changing.
(function (root) {
  "use strict";

  const ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;" };
  const escape = text => text.replace(/[&<>"]/g, c => ESCAPES[c]);

  // The key of square (x, y) in the coordinates of a player's chunk, as the
  // server's game.viewKey gives it.
  const viewKey = (legend, x, y) => (y + legend.height) * 3 * legend.width + x + legend.width;

  // The map's HTML for `state`, a `map` event's data, and `legend`, the
  // `legend` event's: each row as runs of squares that look alike, a run
  // with a class in a span, rows ending in a line feed but the last.
  function mapHtml(state, legend) {
    const sight = new Set(state.sight);
    const near = new Map(state.near.map(([key, cls, title]) => [key, { cls, title }]));
    // How square `c` at (wx, wy) in world coordinates looks: its class,
    // dimmed out of sight, and for a monster or a person, a name.
    const look = (c, wx, wy) => {
      const x = wx - state.cx * legend.width;
      const y = wy - state.cy * legend.height;
      if (c === "@") {
        if (x === state.you[0] && y === state.you[1]) return { cls: "you", title: null };
        return near.get(viewKey(legend, x, y)) || { cls: "ally", title: null };
      }
      if (c >= "A" && c <= "Z") return { cls: "monster", title: legend.monsters[c] || "monster" };
      const kind = c === "." && state.depth === 0 ? "ground" : legend.classes[c];
      if (kind === undefined || kind === null) return { cls: null, title: null };
      return { cls: sight.has(viewKey(legend, x, y)) ? kind : kind + " seen", title: null };
    };
    const run = (style, text) => {
      if (!style.cls) return escape(text);
      const title = style.title ? ` title="${escape(style.title)}"` : "";
      return `<span class="${escape(style.cls)}"${title}>${escape(text)}</span>`;
    };
    const same = (a, b) => a.cls === b.cls && a.title === b.title;
    let html = "";
    state.rows.forEach((row, y) => {
      const wy = state.top + y;
      let start = 0;
      let current = look(row[0], state.left, wy);
      for (let x = 1; x < row.length; x++) {
        const next = look(row[x], state.left + x, wy);
        if (!same(next, current)) {
          html += run(current, row.slice(start, x));
          start = x;
          current = next;
        }
      }
      html += run(current, row.slice(start));
      if (y < state.rows.length - 1) html += "\n";
    });
    return html;
  }

  root.crawlMapHtml = mapHtml;
  if (typeof customElements === "undefined") return;

  let legend = null;
  let state = null;

  // The map element: a <pre class="map"> it fills, which the page measures
  // to size the window, kept drawn from the latest data when the game's
  // HTML around it is replaced.
  class CrawlMap extends HTMLElement {
    connectedCallback() { this.draw(); }

    draw() {
      let pre = this.querySelector("pre.map");
      if (!pre) {
        pre = document.createElement("pre");
        pre.className = "map";
        this.appendChild(pre);
      }
      if (legend && state) pre.innerHTML = mapHtml(state, legend);
    }
  }
  customElements.define("crawl-map", CrawlMap);

  const redraw = () => document.querySelectorAll("crawl-map").forEach(map => map.draw());
  root.crawlMap = {
    legend(data) { legend = data; redraw(); },
    update(data) { state = data; redraw(); },
  };
})(globalThis);
