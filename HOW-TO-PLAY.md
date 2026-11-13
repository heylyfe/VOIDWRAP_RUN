# VOIDWRAP RUN — How to play

> **Quick start (4 steps)**
>
> 1. Open https://github.com/heylyfe/VOIDWRAP_RUN/releases in your web browser.
> 2. Under the newest version, click "Assets", then click the file whose name ends in `-windows.zip` (it is about 2–3 MB).
> 3. In your Downloads folder, right-click that file, choose "Extract All...", click "Extract", then open the folder `VOIDWRAP_RUN` inside it.
> 4. Double-click PLAY. If a blue box says "Windows protected your PC", click "More info", then click "Run anyway". Your web browser opens the game after a moment.

## What is this?

VOIDWRAP RUN is a fast 3D platformer that runs in your web browser.

It is a pre-release ALPHA [an early test version]: things will change, and there may be bugs.

Official tagline: "Collect every gift. Outrun what you voted for."

## Download and start (Windows)

### Step 1: Download the game

1. Open your web browser.
2. Type `https://github.com/heylyfe/VOIDWRAP_RUN/releases` into the address bar at the top and press Enter. (If this text is clickable where you read this, you can click it instead.)
3. You now see a list of versions of the game. The newest version is at the top. It has a small label "Pre-release".
4. Under the top version, find the word "Assets" and click it. A list of files opens.
5. Click the file whose name ends in `-windows.zip`, for example `VOIDWRAP_RUN-v0.1.0-alpha.1-windows.zip`.
6. The browser saves the file. It is about 2–3 MB, so this takes only a few seconds.
7. The browser saves it into your **Downloads** folder. You may also see the file appear at the bottom of the browser window.

### Step 2: Unzip the file

The download is a zip file [a packed folder, like a sealed envelope]. You must unpack it first. Running the game from inside the zip without extracting does **not** work.

1. Open the **Downloads** folder on your computer.
2. Find the zip file. Its name ends in `-windows.zip`.
3. RIGHT-click the zip file (press the right-hand mouse button once). A menu opens.
4. Click "Extract All..." in the menu.
5. A window opens. Click the button "Extract".
6. A normal folder opens. It is named `VOIDWRAP_RUN-...` (for example `VOIDWRAP_RUN-v0.1.0-alpha.1`).
7. Open that folder. Inside it there is another folder named `VOIDWRAP_RUN`. Open that one too.

Inside the `VOIDWRAP_RUN` folder you should see: `PLAY`, `PLAY-WITH-FRIENDS`, `voidwrap-server.exe`, a folder named `web`, and this guide.

Two important notes:

- Windows may hide the ".cmd" ending, so the files may just show as "PLAY" and "PLAY-WITH-FRIENDS". That is normal.
- Do not move files out of this folder and do not delete the `web` folder. The game will not start without them.

### Step 3: Start the game

1. Double-click `PLAY` (it may show as PLAY or PLAY.cmd).
2. The FIRST time, Windows may show a blue box that says "Windows protected your PC". This appears for new games from small developers that are not "signed" [not registered with a paid certificate]. It does not mean the game is dangerous.
3. In that blue box, click "More info" (a small line of text), then click "Run anyway".
4. A black window (the "console") opens with some text in it. **KEEP IT OPEN while you play.** Closing this window stops the game. You can make it small, but do not close it.
5. Your web browser opens the game by itself after a moment. If it does not: open your browser (Chrome, Edge or Firefox), type `localhost:7878` into the address bar at the top, and press Enter.
6. To stop playing: close the browser tab first, then close the black window.

Good to know: next time you do not need to download anything again. Just open the `VOIDWRAP_RUN` folder and double-click PLAY.

A note for some Windows 11 computers: "Smart App Control" can block the game completely, with no "Run anyway" button. In that case the game cannot run on that PC right now. Turning Smart App Control off is possible in Windows Security, but it cannot be turned back on without resetting Windows — only do that if you understand what it means.

## Linux

1. On the same releases page, click the file whose name ends in `-linux.tar.gz`.
2. Extract it.
3. Open a terminal in the `VOIDWRAP_RUN` folder.
4. Type `./play.sh` and press Enter. To host a game for friends, type `./play.sh friends` instead.

## Controls

First: click once inside the game so it can use your mouse. The mouse pointer disappears — that is normal. Press Esc to get the pointer back.

| Action | How |
| --- | --- |
| Move | W A S D (or the arrow keys) |
| Look around | Move the mouse |
| Jump | Space — hold it for a higher jump, tap it for a small hop |
| Dash (a quick burst forward) | Shift or the right mouse button |
| Choose a card on the selection screen | Number keys 1–9 |
| Confirm on the selection screen | Enter |
| Get the mouse pointer back | Esc |

## How a run works

1. You start in the LOBBY, standing on a big checkered platform. A screen with cards appears.
2. "WHO HUNTS YOU — choose 2": pick 2 enemy cards. Each card explains how that enemy moves and how dangerous it is (red dots = threat). These enemies will chase you during the run.
3. "TAKE ONE FROM THE VOID — choose 1": pick 1 card. Blue cards (UPGRADE) help you. Red cards (CURSE) make the run harder.
4. Click a card to choose it. Click it again to un-choose it. Then click CONTINUE (or press Enter).
5. A new, randomly made level appears. Everyone starts on the big platform.
6. Collect the 5 gold gifts with the pink ribbons. The counter at the bottom shows how many you have, for example "2 / 5". Touching a gift collects it.
7. When all 5 are collected, golden bonus gifts appear (they are money for later) and the exit turns on. The exit is a tall pillar of light, called the "beacon". Walk into the light to finish the level.
8. You go back to the lobby, pick a new card, and the next level starts. The enemies stay the same for the whole run.
9. If you fall off into the void or an enemy touches you, the run ends — unless you picked an extra life. Click "Run again" to start a new run.

Good to know:

- Enemies are asleep for the first few seconds of a level. Use that time to get ahead.
- The box in the top-right corner shows your money, your lives, your enemies and your cards. Point at a name to see what it does.

## Playing with friends (same home network)

Friends must be on the same Wi-Fi/network. They do NOT need to download anything. Playing together over the internet (not on the same network) is not supported in this alpha yet.

1. One person is the "host". The host double-clicks `PLAY-WITH-FRIENDS` instead of `PLAY`.
2. Windows may ask "Allow access?" (this box comes from Windows Defender Firewall). Click "Allow access" — otherwise your friends cannot join.
3. The black window shows one or more addresses like `http://192.168.1.23:7878`.
4. Your friends open a browser on their own computers, type the address from YOUR black window into the address bar at the top, and press Enter.
5. Up to 8 players can play. Everyone votes on the cards together. The level starts when everybody has pressed Continue.

If you reach the beacon (or run out of lives) before the others, you become an invisible SPECTATOR until they finish. As a spectator you fly:

- W A S D to fly, Space to go up, C to go down, Shift to fly faster
- Press F to follow another player
- Press G to fly freely again

## Something went wrong?

**Windows shows "Windows protected your PC"**
Click "More info", then click "Run anyway". See "Download and start (Windows)", Step 3.

**The black window opens and closes immediately**
You are probably running the game from inside the zip. Extract it first — see "Download and start (Windows)", Step 2.

**The browser shows "This site can't be reached"**
The black window is closed or still starting. Wait a few seconds, make sure the black window is open, then refresh the page (press F5).

**The black window says the address is already in use**
The game is already running in another black window. Close the extra one, or restart the computer.

**The game picture is black or very slow**
Update your browser. Use Chrome or Edge. Close other heavy programs. The bottom-left corner of the game shows "fps" (frames per second) — 60 is smooth.

**Friends cannot join**
Check all of these:
- Your friends are on the same Wi-Fi/network as you.
- You clicked "Allow access" when Windows asked.
- They typed the address shown in YOUR black window (not `localhost`).
- Your antivirus or firewall may need to allow `voidwrap-server.exe`.

**You found a bug**
Please report it at https://github.com/heylyfe/VOIDWRAP_RUN/issues — click "New issue" and describe what happened.

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
