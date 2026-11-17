# gpui-fast source

`gpui-fast` 0.1.15 as published on crates.io, with the local changes below, recorded by
`scripts/vendor-gpui-fast.py --published 0.1.15`. `SOURCE.json` records the archive's
SHA-256, every upstream file hash and the original and patched hash of each changed
file. The package-level `Cargo.lock` is not vendored. The other gpui-fast crates
(`-macos`, `-linux`, `-windows`, `-wgpu`, `-web`, `-platform`, `-macros`) come from
crates.io 0.1.15 unchanged.

Released in 0.1.15 and no longer patched here: a `measure_all` list measured over later
frames after a width change (#129, upstream's `src/fast/list_measure.rs`).

Released in 0.1.14 and no longer patched here (each was a Material PR):
wheel at its scroll limit (#104), released entities in a spliced view (#106),
hover freeze for listeners (#105), `record_entities_accessed` by what entered and
left (#114), `em` metrics through fallback shaping (#115), lazy tab-stop index
(#113, upstream's `src/fast/tab_stop.rs`; `src/tab_stop.rs` is unused upstream code).

The accessibility subtree patch below adds public helper methods; earlier profiler
and text patches add no API. `src/profiler.rs` and `src/profiler/journal.rs` use
`try_update` in place of the deprecated `fetch_update` (identical semantics), so
deny-warnings builds cover the crate.

Test harness: `src/style.rs` (`#[perf]` to `#[test]`), `src/svg_renderer.rs` and
`tests/fonts/` (fonts the package omits), `examples/image/` (a fixture a
`src/platform.rs` test reads), and `src/profiler/hang.rs`'s
`slow_views_are_journaled_with_their_own_time`, which takes the draw that contains the
slow views rather than the last one: a later frame can replay them without the work.

Behavior, not yet released upstream: `src/text_system/line_layout.rs`,
`src/text_system/line_wrapper.rs` hang a space that overflows after content
instead of starting the next wrapped row with it (`vendor/gpui-pre/hanging-space-patch.md`; zed-industries/zed#66644).
`src/fast/layers/wheel_latch.rs` (new), `src/elements/div.rs`, `src/elements/list.rs`,
`src/fast/layers/input.rs`, `src/fast/layers/mod.rs`: nested scroll containers. Every
container under the pointer moved by the same wheel, so a list inside a scrolling page
moved with the page. The innermost container the wheel moves now takes the event, and
the scroll containers around it leave it alone (the event still propagates, so other
wheel listeners, a container's own `on_scroll` among them, hear it); it keeps
the gesture at the same pointer position while a trackpad's fingers are down, and after
they lift, or for a mouse wheel, while events keep coming less than 250 ms apart (by the
executor's clock), until a new touch: at its end it holds the rest of the swipe and its
momentum instead of handing them to the page, and a page scrolling under a still pointer
is not taken over by a list that comes under it. The containers under the pointer note
themselves in the capture phase, so a container knows whether the one keeping the
gesture is among them. A container scrolling on one axis takes only a wheel going mainly
along it; one going mainly across it (a vertical swipe over a horizontal strip, even with
a little sideways movement) moves it as before without taking the event, so the page
scrolls too. Tests: `src/fast/tests/wheel_latch.rs`. The upstream backend (`gpui-pre`) keeps
upstream's wheel behavior.

Accessibility API and initial tree capture (`src/window.rs`, `src/window/a11y.rs`,
`src/fast/a11y.rs`; identical surface in `vendor/gpui-pre/a11y-subtree-patch.md`):
- `register_initial_capture_handle`, `unregister_initial_capture_handle`,
  `capture_initial_tree_for_window`, and `Window::capture_initial_a11y_tree` draw a window
  with `suppress_platform_update` when accessibility activates and return its initial
  `TreeUpdate` directly instead of queuing a delayed frame update.
- `A11y::note_focus_without_node` skips logging when `focus_id` already maps to a live node.
- `Window::set_a11y_focus_handle` associates a `FocusHandle` with an element's
  `NodeId`.
- `A11ySubtreeBuilder::{parent_id, set_node_children, node, node_mut}` let callers
  read and update accessibility nodes inside a subtree, and
  `A11yLog::preserve_children` preserves retained Fast log children when
  `set_node_children` updates a node.

Patched `gpui-base` readable text views call `Window::set_a11y_focus_handle` and
`A11ySubtreeBuilder::{parent_id, set_node_children}`. Drop this patch once upstream
`gpui-fast` releases equivalent APIs and initial tree capture.

## Post-action accessibility flush

`src/window.rs` handles each queued accessibility action in its own App update.
A second update runs one pending next-frame callback batch so notifications flush
before `Window::flush_a11y_after_action` checks frame demand. The receiver calls the
helper at most twice for an enabled, active accessibility tree in a live window
outside a draw. Each pass has its own App update so focus listeners and deferred
notifications flush before the next check. Each pass clears its element arena and
publishes the updated `TreeUpdate` without presentation.
Pending presentation schedules a platform frame. Remaining dirty state or callbacks
also wake the platform. Callbacks registered during drawing wait for a later frame.
The platform action callback only queues requests because AccessKit can hold a tree
borrow during that callback. Display-link pacing and input dispatch stay unchanged.
Remove this patch when both engines publish post-action trees before the next tick.
