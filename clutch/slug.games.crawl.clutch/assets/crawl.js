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
  }
});
// The server pushes the game's HTML after each tick that changes it.
const events = new EventSource("/events");
events.addEventListener("game", e => {
  const game = document.getElementById("game");
  game.innerHTML = e.data;
  htmx.process(game);
});
