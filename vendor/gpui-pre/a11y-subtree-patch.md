# Accessibility initial tree capture and subtree helpers

Source: the published Apache-2.0 `gpui-pre` 0.3.12 package (`SOURCE.json`). Files:
`src/window.rs`, `src/window/a11y.rs`. Identical public API in `vendor/gpui-fast`.

**Why.** Platform accessibility activation previously signaled an async channel instead of
returning an initial `TreeUpdate`, so assistive clients missed the initial tree until a
later redraw. Patched `gpui-base` readable text views also need to attach synthetic
document nodes and register focus handles inside an existing subtree.

**What.**
- `Window::a11y_init` registers the window via `a11y::register_initial_capture_handle`
  and calls `a11y::capture_initial_tree_for_window` during activation to draw with
  `suppress_platform_update` and return the captured `TreeUpdate`. `remove_window`
  clears the handle via `a11y::unregister_initial_capture_handle`.
- `A11y::note_focus_without_node` returns early when `focus_id` already maps to a live
  node in `self.nodes`.
- `Window::set_a11y_focus_handle` registers a `FocusHandle` on an element's
  accessibility node and marks it focused when `focus_handle.is_focused(self)` is true.
- `A11ySubtreeBuilder::{parent_id, set_node_children, node, node_mut}` inspect and
  replace node children inside an open subtree. `gpui-fast` adds
  `A11yLog::preserve_children` so retained Fast frames preserve updated child lists.

**Removal.** Drop this patch once upstream `gpui-pre` and `gpui-fast` expose these APIs
and `gpui-kit` updates to them.

Consumers that include the patched `gpui-base` crate need this patch.
