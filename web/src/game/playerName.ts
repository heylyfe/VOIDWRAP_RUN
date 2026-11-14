const KEY = "voidwrap.playerName";
const MAX_LEN = 16;

function randomName(): string {
  return `Player-${Math.floor(1000 + Math.random() * 9000)}`;
}

/** The saved name, or a fresh "Player-XXXX" (saved too, so it stays stable). */
export function loadPlayerName(): string {
  try {
    const saved = window.localStorage.getItem(KEY)?.trim();
    if (saved) return saved.slice(0, MAX_LEN);
  } catch {
    // storage unavailable: fall through to a random name
  }
  const name = randomName();
  savePlayerName(name);
  return name;
}

export function savePlayerName(name: string): void {
  try {
    window.localStorage.setItem(KEY, name.slice(0, MAX_LEN));
  } catch {
    // ignore
  }
}
