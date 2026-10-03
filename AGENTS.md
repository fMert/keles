# Instructions for agents working on Keles

These are the project owner's rules from the original request. Later, explicit
requests can add features to the initial scope. Keep the constraints below
unless the owner explicitly changes them.

## Goal and initial scope

- Build an extremely lightweight, extremely simple browser FPS foundation.
- The initial scene is three-dimensional: one ground plane and a few simple
  colored, low-poly boxes or obstacles.
- The player moves with WASD and looks around with the mouse.
- The weapon is a flat 2D image fixed at the bottom center, not a 3D model.
- Keep a simple working render loop.
- Realistic graphics and visual polish are not required.
- The initial milestone excluded mechanics, enemies, missions, inventory, and
  in-game items. Add such features only when the owner requests them.

## Non-negotiable implementation constraints

1. Do not author JavaScript, directly or indirectly, for the browser. The
   automatically generated wasm-bindgen glue is the sole JavaScript exception.
   A minimal hand-written HTML page may invoke that generated loader through a
   `type="module"` script, as the owner explicitly allowed.
2. All implementation code authored by an agent must be pure Rust. Borrowed
   source may originate in C, C++, Zig, Odin, or another language if it can be
   compiled to WASM, linked, ported cleanly, or used as a reference; any code
   the agent writes or ports must still be Rust.
3. Keep the code extremely simple. Avoid unnecessary abstractions, traits,
   modules, and files. Prefer one Rust file or very few files.
4. The game is shared under the GPL. Use only GPL-compatible borrowed code and
   dependencies, such as MIT, Apache-2.0, BSD, MPL, or GPL-family licenses.
5. Minimize agent-authored code. Look for a suitable existing open-source
   implementation before writing a function yourself.
6. When a suitable project exists, borrow only the needed function, module, or
   snippet; do not copy the entire project.
7. Put a source comment at the beginning of every borrowed part. State the
   repository, file path, exact line range, and license.

## Open-source quality bar

- Reject repositories that look like a one-prompt LLM dump or other low-quality
  “vibe-coded” work.
- Warning signs include only one or two bulk commits; no tests, CI,
  documentation, or examples; generic or absent README; copy-pasted
  boilerplate; inconsistent or non-idiomatic code; no maintainer engagement,
  issue triage, or real users.
- AI assistance on a mature, real project is acceptable. Judge the project as
  a whole.
- Prefer long, meaningful commit histories, multiple contributors, real
  downstream users or dependents, references from serious projects, clear
  architecture, documentation, tests, examples, and active or historically
  solid maintenance.
- When quality is uncertain, cross-check commit history, contributor count,
  issue tracker, and downstream usage on GitHub, crates.io, or similar sources.
- For **each** borrowed piece, report its repository name and URL, license,
  exact file/function/line range, and why the project meets this quality bar.

## Prefer existing tools and libraries

- Use wasm-bindgen or wasm-pack for WebAssembly compilation instead of building
  that tooling.
- Use `web-sys` for browser canvas, WebGL, window, and input event bindings;
  do not write JavaScript for them.
- Use a mature math crate such as `glam` or `nalgebra`; do not write a matrix
  library.
- Render through WebGL2 or WebGPU from Rust. Evaluate an established
  open-source crate for the render pipeline first. If `wgpu` is too heavy for
  this simple project, direct WebGL calls through `web-sys` are acceptable;
  look for a suitable minimal wrapper such as `glow` before writing one.

## Output and workflow

- Produce a browser-runnable `.wasm` file using wasm-pack or wasm-bindgen.
- Keep the HTML loader minimal and hand-written. It must contain no
  agent-authored JavaScript; invoking the generated wasm-bindgen loader is the
  required exception.
- For the original first deliverable, the required files were Cargo and
  wasm-pack configuration, the HTML loader, and Rust code for camera
  position/yaw/pitch, WASD and mouse look, ground and 3–5 boxes, a bottom-center
  2D weapon placeholder, and the render loop.
- Work in this order for new implementation needs: research suitable
  open-source crates/snippets and check their quality; present a simple file
  plan; implement in Rust with source comments for borrowed parts; compile
  with a command such as `wasm-pack build --target web`; run on localhost and
  verify in the browser; report briefly what worked, what did not, and a
  reasonable next step.
- The owner required the original research findings before coding and then
  approved that first implementation. Do not repeat that approval request for
  work already authorized.
- Keep the number of files, dependencies, and lines of agent-authored code as
  low as practical. Simplicity takes priority over visual appeal.
