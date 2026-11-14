# VOIDWRAP RUN — How to play

> **Quick start (4 steps)**
>
> 1. Open https://github.com/heylyfe/VOIDWRAP_RUN/releases in your web browser.
> 2. Under the newest version, click "Assets", then click the file whose name ends in `-windows-setup.exe`. This is the installer — the recommended way to play.
> 3. Double-click the downloaded file. If a blue box says "Windows protected your PC", click "More info", then click "Run anyway".
> 4. In the installer, click "Next", then "Install". At the end, click "Finish". If "Start VOIDWRAP RUN now" is ticked, the game opens in its own window after a moment.

## What is this?

VOIDWRAP RUN is a fast 3D platformer. It runs in its own window on your computer — you do not need a web browser to play.

It is a pre-release ALPHA [an early test version]: things will change, and there may be bugs.

Official tagline: "Collect every gift. Outrun what you voted for."

## Download and start (Windows)

There are two ways to get the game on your computer. The installer is easier and is the recommended way. The zip file works exactly as it did before.

### Step 1: Download the game

1. Open your web browser.
2. Type `https://github.com/heylyfe/VOIDWRAP_RUN/releases` into the address bar at the top and press Enter. (If this text is clickable where you read this, you can click it instead.)
3. You now see a list of versions of the game. The newest version is at the top. It has a small label "Pre-release".
4. Under the top version, find the word "Assets" and click it. A list of files opens.
5. Two files work on Windows:
   - the installer, whose name ends in `-windows-setup.exe` [a small program that puts the game on your computer for you]. This is the recommended way.
   - the zip file, whose name ends in `-windows.zip`.
6. Click the one you want. The browser saves it into your **Downloads** folder. You may also see the file appear at the bottom of the browser window.

### Step 2: Install the game (recommended way)

1. Open the **Downloads** folder on your computer.
2. Double-click the file whose name ends in `-windows-setup.exe`.
3. The FIRST time, Windows may show a blue box that says "Windows protected your PC". This appears for new games from small developers that are not "signed" [not registered with a paid certificate]. It does not mean the game is dangerous. In that blue box, click "More info" (a small line of text), then click "Run anyway".
4. The installer window opens. Click "Next", then "Install". It needs no administrator rights, and it installs only for you, the current user.
5. If you want a shortcut on your desktop, leave the box for it ticked.
6. At the end, click "Finish". If "Start VOIDWRAP RUN now" is ticked, the game opens right away.
7. After installing, the game is in your Start menu as **VOIDWRAP RUN**, with its own game icon. The guides `HOW-TO-PLAY.md` (this file) and `ENEMIES.md` are in the game folder.
8. To remove the game later: open **Windows Settings > Apps**, find VOIDWRAP RUN, and click "Uninstall". You can also uninstall it from the Start menu.

### Step 2 (alternative): the zip file

The download is a zip file [a packed folder, like a sealed envelope]. You must unpack it first. Running the game from inside the zip without extracting does **not** work.

1. Open the **Downloads** folder on your computer.
2. Find the zip file. Its name ends in `-windows.zip`.
3. RIGHT-click the zip file (press the right-hand mouse button once). A menu opens.
4. Click "Extract All..." in the menu.
5. A window opens. Click the button "Extract".
6. A normal folder opens. It is named `VOIDWRAP_RUN-...` (for example `VOIDWRAP_RUN-v0.3.1-alpha.1`). Open it.

Inside you should see: `VOIDWRAP RUN.exe` (the game), `PLAY-IN-BROWSER.cmd` (a backup way to start), a folder named `web`, and the guides (`HOW-TO-PLAY.md` and `ENEMIES.md`).

Two important notes:

- Windows may hide the ".exe" and ".cmd" endings, so the files may just show as "VOIDWRAP RUN" and "PLAY-IN-BROWSER". That is normal.
- Do not move files out of this folder and do not delete the `web` folder. The game will not start without it.

### Step 3: Start the game

1. If you used the installer: click the Windows Start button and click **VOIDWRAP RUN**, or double-click the desktop shortcut if you made one.
2. If you used the zip: open the extracted folder and double-click `VOIDWRAP RUN` (it may show as "VOIDWRAP RUN" or "VOIDWRAP RUN.exe"). If the blue box "Windows protected your PC" appears and you have not clicked through it yet, click "More info", then click "Run anyway".
3. The game opens in its OWN window. There is no black window and no web browser involved.
4. You now see the title screen with three buttons: **PLAY** (play alone, or with friends who already joined), **PLAY WITH FRIENDS**, and **JOIN A FRIEND**. Click PLAY to start.
5. To stop playing: close the game window. That is all.

If the game window stays empty or white, or does not open at all: double-click `PLAY-IN-BROWSER` instead. A black window (the "console") opens with some text in it. **KEEP IT OPEN while you play.** Closing this window stops the game. The game opens in your web browser after a moment. If it does not: open your browser (Chrome, Edge or Firefox), type `localhost:7878` into the address bar at the top, and press Enter.

Good to know: next time you do not need to download anything again. With the installer, start the game from the Start menu. With the zip, open the folder and double-click VOIDWRAP RUN.

A note for some Windows 11 computers: "Smart App Control" can block the game completely, with no "Run anyway" button. In that case the game cannot run on that PC right now. Turning Smart App Control off is possible in Windows Security, but it cannot be turned back on without resetting Windows — only do that if you understand what it means.

## Linux

1. On the same releases page, click the file whose name ends in `-linux.tar.gz`.
2. Extract it.
3. Open a terminal in the `VOIDWRAP_RUN` folder.
4. Type `./play.sh` and press Enter. To host a game for friends, type `./play.sh friends` instead.

## Controls

First: click once inside the game so it can use your mouse. The mouse pointer disappears — that is normal. Press **Ctrl** to free the pointer again. Press **Ctrl** once more to lock it — or just click inside the game. The game shows a small hint about the mouse at the bottom of the screen.

| Action | How |
| --- | --- |
| Move | W A S D (or the arrow keys) |
| Look around | Move the mouse |
| Jump | Space — hold it for a higher jump, tap it for a small hop |
| Class ability | Shift or the right mouse button (dash for the Striker, grapple for the Grappler, the rooted push for the Bulwark, glide for the Glider) |
| Use an altar | E |
| Choose a class (first lobby) | Number keys 1–4 |
| Choose a card on the selection screen | Number keys 1–9 |
| Confirm on the selection screen | Enter |
| Lock or free the mouse pointer | Ctrl |

## How a run works

1. You start in the LOBBY, standing on a big checkered platform. Level 1 is a clean start: no hunters, no upgrades, no curses. The screen says "CHOOSE A CLASS": press a number key from 1 to 4, or click a card. The class stays for the whole run.
2. The four classes — each has an ability on Shift or the right mouse button:
   - **Striker**: dash — a short burst forward.
   - **Grappler**: fires an auto-aimed grapple that pulls you to the platform ahead. Keep moving toward it while you press.
   - **Bulwark**: on the ground, presses Shift or the right mouse button to ROOT you in place for 1.6 seconds. While rooted you cannot move, but a pulse pushes everything nearby away, and almost nothing can hurt you — falling shells still can. Long cooldown: 16 seconds.
   - **Glider**: one extra jump in mid-air. Hold Shift while falling to glide slowly down.
3. Click CONTINUE (or press Enter) when you have chosen.
4. From level 2 on, the lobby shows cards:
   - "WHO HUNTS YOU — choose 2" on the first card lobby: pick 2 hunter cards. Each card explains how that hunter moves and how dangerous it is (red dots = threat). These hunters will chase you during the run.
   - "TAKE ONE FROM THE VOID — choose 1" on every lobby after that: pick 1 card. Blue cards (UPGRADE) help you. Red cards (CURSE) make the run harder.
5. Click a card to choose it. Click it again to un-choose it. Then click CONTINUE (or press Enter).
6. Below the cards is the LOBBY SHOP. Scroll down to see it. Items cost gold from the golden gifts you collect — the money is shared by the whole team. Each item shows its price and how many are left. The items are: an extra life, a "reroll" that swaps the reward cards, and small permanent boosts. The Buy button explains when you cannot afford something.
7. A new, randomly made level appears. The maps are now much bigger (see "Big maps" below). Everyone starts on the spawn platform.
8. PHASE 1 — collect every small gift on the level. There are about 50 on level 1 and more on every later level (up to 400). Touching a gift collects it. The counter at the bottom shows how many you have, for example "12 / 50".
9. PHASE 2 — when the LAST gift is collected, the map collapses and the beacon on your spawn platform lights up (see "Two phases: collect, then run" below). Walk into the beacon to finish the level.
10. You go back to the lobby, pick a new card, maybe buy something, and the next level starts. The hunters stay the same for the whole run. From level 4 on, some levels announce a MILESTONE: more hunter copies, shorter warnings before tiles fall, or thicker fog.
11. Some levels have an EVENT, announced at the start of the level:
    - **High Rise**: the platforms climb higher and higher.
    - **Slick Ice**: you slide and stop slowly.
    - **Crumbling**: cracked platforms fall away a moment after you step on them. They come back after 3 seconds.
12. Some levels have an ALTAR: a small stone shrine with a glowing sign. Walk up to it and press E. You get one random upgrade AND one random curse. Each altar works only once.
13. The hunters you voted for join the chase (see "Hunters" below). Hunters sleep for the first 4 seconds of a level.
14. If you fall off into the void or a hunter catches you, the run ends — unless you picked an extra life. Click "Run again" to start a new run.

Good to know:

- Use the first 4 seconds of a level — while the hunters sleep — to get ahead.
- The box in the top-right corner shows your money, your lives, your hunters and your cards. Point at a name to see what it does.
- If the game window stays empty or white, use `PLAY-IN-BROWSER` instead (see "Download and start (Windows)", Step 3).

## Big maps

The maps are much bigger now. They are built from hand-made pieces that the game puts together:

- several walkway layers stacked on pillars, joined by ramps;
- ramps you walk up and down at full speed — you never need to jump;
- tall towers above everything;
- bridges with a second road on top;
- glowing jump pads that throw you about 4 metres into the air;
- paths that loop back and split at junctions.

Small gifts line the paths. Every map is checked before you play it, so every gift and the beacon can always be reached.

## Two phases: collect, then run

Every level has two phases.

**PHASE 1 — COLLECT.** Small gifts sit on the platforms — about 50 on level 1, more on every later level, up to 400. Walk into them to collect. The counter at the bottom shows your progress, for example "12 / 50".

**PHASE 2 — RUN.** When you collect the last gift, a warning starts: the map begins to collapse in 3 seconds, and the banner says "The void hungers — take the gold and run". Then the map collapses from the OUTSIDE IN — tiles far away fall first. A tile shakes and flashes before it falls. The spawn platform never falls.

Where you collected a gift, a golden gift appears — each one is 1 gold for the shared team money. Grab what you can while the ground falls away behind you. Then walk into the BEACON: a tall pillar of light on the spawn platform. It finishes the level for the whole team.

Golden gifts on tiles that have already fallen are lost. Get to them before the floor gives way.

## Hunters

The hunters you vote for in the lobby chase you and try to stop you. All nine have been reworked:

- Every deadly attack is announced first: a visible warning — a ring, a line or a flare — appears 0.6 to 2.5 seconds before it lands. The same attack always uses the same shape, so you learn what each shape means.
- Some hunters cannot kill you at all. They push you around or slow you down instead. The void below does the rest — falling is still falling.

The line-up:

- **The Pallbearer** — follows the trail you left seconds ago. Never go back the way you came.
- **Snapshrike** — locks a line and dashes down it. Sidestep the moment the line stops moving.
- **Moteling** — marks the fastest-moving player. Stand still until its ring snaps shut.
- **The Rift Warden** — pushes everything away from itself. It cannot hurt you; the fall can.
- **Gale Wight** — drops exploding shells marked by circles on the floor. Standing under a bridge makes the shell break early.
- **Lantern Leech** — rushes to wherever a gift was just collected. Grab the gift, then step away.
- **Bolt Sentry** — never moves; shoots along a thin aiming line. Break its line of sight, or jump the bolt.
- **False Gift** — looks exactly like a gift, then lunges. Watch for the magenta shimmer.
- **Crumble Wisp** — makes the floor around it slow you and lock your abilities. Cross fast, do not stand there.

The full guide — what each hunter does, its exact warning, and how to survive — is `ENEMIES.md`, in the game folder.

## Playing with friends

Friends need nothing installed except a web browser — or the game itself if they want the game window. The host's computer runs the game for everyone. Friends can be on your Wi-Fi, or anywhere else (see below).

You are the "host":

1. On the title screen, click **PLAY WITH FRIENDS**.
2. Switch on **"Let friends join"**. The FIRST time, Windows may ask "Allow access?" (this box comes from Windows Defender Firewall). Click "Allow access" — otherwise your friends cannot join.
3. The panel now shows addresses:
   - **"FRIENDS ON YOUR WI-FI"**: an address like `http://192.168.1.23:7879`. This is for friends on the same Wi-Fi/network as you. Click the **Copy** button to copy it.
   - **"FRIENDS ANYWHERE"**: an address like `http://203.0.113.5:7879`. This one is only shown when the game managed to set up your router automatically (this is called UPnP). Send it to friends who are somewhere else.
4. If the panel says "Your router did not allow automatic setup": friends on your Wi-Fi can still join. For friends somewhere else you must, in your router's settings (ask whoever manages your internet; every router is different), either turn on "UPnP" or "forward" port 7879 (TCP) to your computer. If you turned on UPnP, switch "Let friends join" off and on again - the "FRIENDS ANYWHERE" address should now appear. If you forwarded the port instead, your router's settings page shows your public address (often called "WAN IP" or "Internet IP"); send your friends `http://` + that address + `:7879`.
5. Only share the address with people you trust. Switch "Let friends join" off when you are done.
6. Click **START PLAYING**.

You are a friend:

1. If you do not have the game: open a web browser on your own computer, put the address the host sent into the address bar at the top, and press Enter. That is all you need.
2. If you have the game: click **JOIN A FRIEND** on the title screen, paste the address into the box, and click **Join**.

Up to 8 players can play. Everyone votes on the cards together. The level starts when everybody has pressed Continue.

If you reach the beacon (or run out of lives) before the others, you become an invisible SPECTATOR until they finish. As a spectator you fly:

- W A S D to fly, Space to go up, C to go down, Shift to fly faster
- Press F to follow another player
- Press G to fly freely again

## Something went wrong?

**Windows shows "Windows protected your PC"**
Click "More info", then click "Run anyway". See "Download and start (Windows)", Step 2.

**The game window stays empty or white, or does not open**
Double-click `PLAY-IN-BROWSER` instead. A black window opens and the game opens in your web browser. Keep the black window open — closing it stops the game. (This needs the zip download; if you used the installer and the game window stays white, report it — see below.)

**The black window opens and closes immediately** (when using PLAY-IN-BROWSER)
You are probably running the game from inside the zip. Extract it first — see "Download and start (Windows)", the zip section.

**The browser shows "This site can't be reached"** (when using PLAY-IN-BROWSER)
The black window is closed or still starting. Wait a few seconds, make sure the black window is open, then refresh the page (press F5).

**The black window says the address is already in use**
The game is already running in another window or black window. Close the extra one, or restart the computer.

**The game picture is black or very slow**
Close other heavy programs. The bottom-left corner of the game shows "fps" (frames per second) — 60 is smooth. If you are playing through PLAY-IN-BROWSER: update your browser and use Chrome or Edge.

**Friends cannot join**
Check all of these:
- You switched on "Let friends join" and clicked START PLAYING.
- You clicked "Allow access" when Windows asked.
- Friends on your Wi-Fi used the address under "FRIENDS ON YOUR WI-FI" (not `localhost`).
- Friends elsewhere need the "FRIENDS ANYWHERE" address — which needs UPnP in your router, or port 7879 (TCP) forwarded to your computer.
- Your antivirus or firewall may need to allow the game.

**You found a bug**
Please report it at https://github.com/heylyfe/VOIDWRAP_RUN/issues — click "New issue", describe what happened, and attach the file `voidwrap.log` from your game folder. The game writes that file when something goes wrong.

## Build from source (for developers)

You need:

- Rust, stable version, from https://rustup.rs
- Node.js 24 or newer
- pnpm (install it by typing `npm install -g pnpm` in a terminal)

Then, in the project folder, run these commands one at a time:

1. `rustup target add wasm32-unknown-unknown`
2. `pnpm install`
3. `npm start` — this builds everything and opens the game.

Other useful commands:

- `npm run start:friends` — hosts the game for your network.
- `npm run dev` — development with hot reload [your changes appear immediately while you work].
- Optional: if you have the `just` tool, run `just verify` to run all checks.

## Host online for free (advanced)

The repository includes a `Dockerfile` and a `render.yaml` for Render.com's free plan:

1. Sign in to Render with your GitHub account.
2. Click "New", then "Blueprint", then choose the repository.
3. Render builds and hosts the game for you.

Note: the free plan sleeps [pauses] after about 15 minutes without visitors. The first visit afterwards takes about a minute to wake the game up.
