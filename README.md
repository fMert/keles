# Keles

Build the browser game from the project root:

```sh
wasm-pack build --target web --release
python3 -m http.server 8000
```

Open `http://localhost:8000/`. Choose **Play → Single player** and select
**Camel** or **Test**. Test is the original box map. Camel uses the supplied
Dust 2 model and its textures. Movement follows the model's floors and walls.
Multiplayer always loads Camel; install the rebuilt server binary too, since
it uses the same mesh for collision and shot obstruction.

Settings are available from the main menu and pause menu. Sensitivity uses
Minecraft Java's 0–200% scale and cubic mouse curve: 50% gives 0.05145° per
raw mouse count. Raw input requests unaccelerated pointer movement through the
browser's Pointer Lock API. If the browser rejects that request, disable Raw
input to use regular mouse input. HUD scale changes the text and health bars,
not the weapon image. Show FPS toggles the top-right counter. Show blood effects
controls the small hit and larger death bursts.

Run the small multiplayer relay locally or on a VPS:

```sh
cargo build --release --manifest-path server/Cargo.toml
./server/target/release/keles-server 0.0.0.0:9001
```

For the VPS, copy only `server/target/release/keles-server` after building
for that machine and run `./keles-server 0.0.0.0:9001`. The Cargo build
directory is not needed at runtime.

Choose **Play → Multiplayer**, enter `IP_ADDRESS:9001` and a nickname, and
connect. Each browser loads the same map locally; the server relays positions,
headings, and health. Other players use four transparent 2D views selected
from their facing direction. Click to fire: a shot through a player's 3D
hitbox removes one of three health points, and boxes block shots. Your health
bar is at the bottom left. After three hits a player dies and their model
disappears; reconnecting resets health. The server
accepts up to 16 simultaneous connections and keeps no
persistent game data. If the game page is served over HTTPS, put the relay
behind a TLS reverse proxy and enter a `wss://` address.

Camel asset credit: **"de_dust2 - CS map" by vrchris**
([Sketchfab source](https://sketchfab.com/3d-models/de-dust2-cs-map-056008d59eb849a29c0ab6884c0c3d87)),
offered there under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
`assets/camel_original.glb` is the supplied original. `tools/convert` extracts
its mesh and images into `assets/camel_mesh.bin` and `assets/camel_images/` and
recenters its coordinates. The original textures are displayed without
replacement. The Sketchfab listing is the uploader's license claim; anyone
distributing the game should verify rights to the Counter-Strike-derived asset.
