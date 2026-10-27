// Turns key presses into the game's key names; the page sends only these.
const NAMED = { ArrowLeft: "left", ArrowDown: "down", ArrowUp: "up", ArrowRight: "right", " ": "space", Enter: "enter" };
function gameKey(e) {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  if (/^[a-zA-Z]$/.test(e.key)) return e.key === "i" || e.key === "I" ? null : e.key.toLowerCase();
  return NAMED[e.key] || null;
}
// An htmx trigger filter must return exactly true.
function isGameKey(e) { return gameKey(e) !== null; }
document.addEventListener("keydown", e => { if (gameKey(e)) e.preventDefault(); });
document.addEventListener("htmx:configRequest", e => {
  const t = e.detail.triggeringEvent;
  if (t && t.type === "keydown") {
    e.detail.parameters.key = gameKey(t);
    e.detail.parameters.shift = t.shiftKey;
  } else if (e.detail.elt.hasAttribute("data-direction") && running()) {
    // Run makes the next direction run, then turns itself off.
    e.detail.parameters.shift = true;
    setRunning(false);
  }
});

// The touch controls' Run button.
function running() {
  const run = document.querySelector(".touch .run");
  return run !== null && run.getAttribute("aria-pressed") === "true";
}
function setRunning(on) {
  const run = document.querySelector(".touch .run");
  if (run) run.setAttribute("aria-pressed", on ? "true" : "false");
}
document.addEventListener("click", e => {
  if (e.target.closest(".touch .run")) setRunning(!running());
});
// How many map columns and rows fit the page: as wide as the space beside
// the panels (or the page, when they go below), leaving room for the log.
function mapSize() {
  const map = document.querySelector(".map");
  const game = document.getElementById("game");
  const aside = game && game.querySelector("aside");
  if (!map || !aside) return { cols: 80, rows: 22 };
  const style = getComputedStyle(game);
  const beside = style.flexDirection === "row" ? aside.offsetWidth + parseFloat(style.columnGap || 0) : 0;
  const room = game.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) - beside;
  const probe = document.createElement("span");
  probe.textContent = "0".repeat(10);
  map.appendChild(probe);
  const box = probe.getBoundingClientRect();
  probe.remove();
  const cw = box.width / 10, lh = box.height;
  const cols = Math.floor((room - 30) / cw);
  // Beside the panels, leave room below the map for the log; with the
  // panels below, only for the touch controls, if they show.
  const touch = document.querySelector(".touch");
  const bar = touch && getComputedStyle(touch).display !== "none" ? touch.offsetHeight : 0;
  const reserve = style.flexDirection === "row" ? 330 : 150 + bar;
  const rows = Math.floor((window.innerHeight - reserve) / lh);
  return { cols: Math.max(20, Math.min(80, cols)), rows: Math.max(8, Math.min(22, rows)) };
}

// The server pushes the game's HTML after each tick that changes it, drawn
// through a window the size of the page; a new size reopens the stream.
let events = null, size = "";
function listen() {
  const s = mapSize(), next = s.cols + "x" + s.rows;
  if (next === size && events) return;
  size = next;
  if (events) events.close();
  events = new EventSource("/events?cols=" + s.cols + "&rows=" + s.rows);
  events.addEventListener("game", e => {
    const game = document.getElementById("game");
    game.innerHTML = e.data;
    htmx.process(game);
  });
}
let resizing = null;
window.addEventListener("resize", () => { clearTimeout(resizing); resizing = setTimeout(listen, 250); });
document.addEventListener("DOMContentLoaded", listen);
