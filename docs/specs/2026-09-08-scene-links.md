# Persistent desktop scene and links

This is the first implementation slice for the DriftWM desktop-canvas idea.
Waynote remains the Markdown note store. `core::scene` adds a separate durable
scene graph for relationships that do not belong in note frontmatter:

- `SceneObject` records a stable Waynote object ID, canvas frame, output/
  workspace placement, lock state, and one of the supported object kinds.
- `DriftWindow` stores a compositor ID only as a refreshable runtime hint.
  `ExternalIdentity::DriftWindow` is the reattachment key when DriftWM recreates
  the window.
- `ChromiumTab` is ready for a future browser adapter. DriftWM's compositor IPC
  exposes windows, not tabs, so tab discovery needs a browser extension/native
  messaging bridge and is intentionally not guessed here.
- `Note` points to the existing Waynote Markdown ULID.
- `Image` covers screenshots created through the existing `grim`/`slurp`
  workflow; `Scribble` stores canvas points directly so it remains independent
  of a raster file.
- `SceneGroup` and `SceneLink` are explicit graph records. Groups update each
  member's reverse `group_ids` index; links require existing object endpoints.

The sidecar is JSON at `$XDG_STATE_HOME/waynote/scene.json` (normally
`~/.local/state/waynote/scene.json`). It is atomically replaced and is separate
from both the Markdown note files and `layout.toml`. A missing sidecar is an
empty scene; malformed JSON is reported rather than silently discarded.

## Next integration slice

The next UI slice should add a scene edit mode to the existing layer-shell
surfaces and render links/groups/scribbles without changing the current note
surface contract. The Rust `driftwm-minimap-rs` repository is the compositor
readout/overview client; it should consume this scene sidecar through a shared
model or IPC seam rather than duplicating Waynote's note store.

The persistent model is deliberately ahead of discovery. A later adapter can
upsert live DriftWM windows and Chromium tabs by `ExternalIdentity`, preserving
scene IDs and links while runtime handles change.
