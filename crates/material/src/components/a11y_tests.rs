//! The accessibility tree applications publish: names, states and values
//! of list rows, overlays, carousel items and progress, read from the tree
//! GPUI Fast builds when assistive technology is active.
use super::*;
use gpui::{
    Context, Entity, IntoElement, Render, SharedString, TestAppContext, Window, div, prelude::*, px,
};
use gpui_base::input::{InputState, TextareaState};
use gpui_base::slider::SliderState;
use serde_json::Value;

const MARKER: &str = "material-a11y-names-verified";
/// A dropdown value too long for its field, which shows it in a tooltip.
const CLIPPED_VALUE: &str = "Qwen3-ASR 1.7B, downloaded and ready to transcribe";

struct Screen(Entity<TimePickerState>);
impl Render for Screen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .child(
                ListItem::new("row", "Wi-Fi")
                    .supporting("Connected")
                    .selected(true)
                    .trailing(ListTrailing::Switch(true))
                    .on_click(|_, _, _| {}),
            )
            .child(
                MaterialCarousel::new("carousel", "Photos")
                    .width(412.)
                    .items([CarouselItem::new().label("Beach").supporting("Summer")]),
            )
            .child(
                status_fact_chip("fact", "Checked: passed", StatusKind::Success).child("Checked"),
            )
            .child(
                Stepper::new("stepper", "Setup progress")
                    .steps(["Source", "Service", "Account"])
                    .current(1)
                    .state_labels("Finished", "Current", "Upcoming"),
            )
            .child(
                ExposedDropdown::new("language", "Language")
                    .show_label(false)
                    .required(true)
                    .options(["English", "Chinese"])
                    .selected(Some(1)),
            )
            .child(
                ExposedDropdown::new("speech", "Speech model")
                    .width(120.)
                    .options([CLIPPED_VALUE])
                    .selected(Some(0)),
            )
            .child(
                ExposedDropdown::new("model", "Model")
                    .menu(MaterialMenu::new("models").item(MaterialMenuItem::new("Deep")))
                    .value("Deep"),
            )
            .child(
                MaterialCard::new("toggle-card", CardVariant::Outlined)
                    .selected(true)
                    .accessibility_label("Toggle card")
                    .on_click(|_, _, _| {}),
            )
            .child(
                CardGroup::new("card-group", "Card choices")
                    .card(
                        MaterialCard::new("choice-a", CardVariant::Outlined)
                            .selected(false)
                            .accessibility_label("First choice"),
                    )
                    .card(
                        MaterialCard::new("choice-b", CardVariant::Outlined)
                            .selected(true)
                            .accessibility_label("Second choice"),
                    ),
            )
            .child(
                CustomRadioGroup::new("swatches", "Accent color")
                    .options(["Red", "Green", "Blue"].into_iter().enumerate().map(
                        |(index, name)| {
                            gpui_base::Radio::new(("swatch", index))
                                .accessibility_label(name)
                                .size(px(20.))
                        },
                    ))
                    .selected(Some(2)),
            )
            .child(drop_target("drop", "Upload files", false).size(px(40.)))
            .child(MaterialTooltip::plain("tip", "Save a copy").trigger(div().size(px(20.))))
            .child(progress_with_label("copy", "Copy", 0.25, window, cx))
            .child(indeterminate_progress_with_label("index", "Index", cx))
            .child(
                MaterialDialog::new("dialog")
                    .headline("Discard draft?")
                    .modal(true),
            )
            .child(
                MaterialSideSheet::new("side")
                    .modal(true)
                    .open(true)
                    .headline("Details"),
            )
            .child(TimePicker::new("time", &self.0))
            .child(
                MaterialBottomSheet::new("bottom")
                    .modal(true)
                    .open(true)
                    .label("Filters")
                    .child(div().h(px(100.))),
            )
    }
}

fn time_labels() -> TimePickerLabels {
    let s = SharedString::new_static;
    TimePickerLabels {
        select_title: s("Select time"),
        enter_title: s("Enter time"),
        hour: s("Hour"),
        minute: s("Minute"),
        am: s("AM"),
        pm: s("PM"),
        confirm: s("OK"),
        cancel: s("Cancel"),
        switch_to_input: s("Keyboard"),
        switch_to_dial: s("Dial"),
        number: std::rc::Rc::new(|n| n.to_string().into()),
        two_digits: std::rc::Rc::new(|n| format!("{n:02}").into()),
        separator: s(":"),
    }
}

/// Every node in the published tree.
fn nodes(tree: &str) -> Vec<Value> {
    fn walk(value: &Value, out: &mut Vec<Value>) {
        match value {
            Value::Object(map) => {
                if map.contains_key("role") {
                    out.push(value.clone());
                }
                map.values().for_each(|v| walk(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(
        &serde_json::from_str(tree).expect("the tree is JSON"),
        &mut out,
    );
    out
}

/// The node labelled `label` with `role` (compared case-insensitively).
fn labelled<'a>(nodes: &'a [Value], role: &str, label: &str, tree: &str) -> &'a Value {
    nodes
        .iter()
        .find(|node| {
            node["label"] == label
                && node["role"]
                    .as_str()
                    .is_some_and(|r| r.eq_ignore_ascii_case(role))
        })
        .unwrap_or_else(|| panic!("no {role} named {label:?} in {tree}"))
}

fn a11y_child(test: &str) -> bool {
    if std::env::var("GPUI_A11Y_ACTIVE").as_deref() == Ok("1") {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("components::a11y_tests::{test}"),
            "--nocapture",
        ])
        .env("GPUI_A11Y_ACTIVE", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains(test),
        "the child ran no assertions: {stdout}"
    );
    true
}

fn painted_tree(cx: &mut gpui::VisualTestContext) -> String {
    for _ in 0..3 {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    cx.update(|window, _| window.debug_a11y_tree_json().expect("accessibility on"))
}

struct RowSemantics;
impl Render for RowSemantics {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                ListItem::new("static-row", "Transcriber")
                    .overline("Tools")
                    .supporting("Installed")
                    .trailing_text("1.2")
                    .disabled(true),
            )
            .child(
                ListItem::new("action-row", "Meeting")
                    .overline("Today")
                    .supporting("Ready")
                    .trailing_text("5 min")
                    .disabled(true)
                    .on_click(|_, _, _| {}),
            )
            .child(
                ListItem::new("tree-row", "Lectures")
                    .role(gpui::Role::TreeItem)
                    .aria_expanded(false)
                    .aria_level(2)
                    .aria_position_in_set(3)
                    .aria_size_of_set(7)
                    .selected(true)
                    .on_click(|_, _, _| {}),
            )
            .child(
                ListItem::new("static-tree-row", "Archive")
                    .role(gpui::Role::TreeItem)
                    .aria_expanded(true)
                    .aria_level(1)
                    .aria_position_in_set(1)
                    .aria_size_of_set(2),
            )
            .child(ListItem::new("default-action", "Open").on_click(|_, _, _| {}))
            .child(ListItem::new("unselected-static", "Unselected item").selected(false))
            .child(
                ListItem::new("unselected-action", "Other destination")
                    .selected(false)
                    .on_click(|_, _, _| {}),
            )
            .child(
                ListItem::new("radio-row", "Other radio")
                    .leading(ListLeading::Radio(false))
                    .on_click(|_, _, _| {}),
            )
    }
}

#[gpui::test]
fn list_rows_publish_text_and_optional_structure(cx: &mut TestAppContext) {
    const TEST: &str = "list_rows_publish_text_and_optional_structure";
    if a11y_child(TEST) {
        return;
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);
        cx.set_reduce_motion(true);
    });
    let (_, cx) = cx.add_window_view(|_, _| RowSemantics);
    let tree = painted_tree(cx);
    let all = nodes(&tree);
    let static_row = labelled(&all, "listItem", "Transcriber", &tree);
    assert_eq!(static_row["description"], "Tools\nInstalled\n1.2", "{tree}");
    assert!(
        static_row.get("selected").is_none_or(Value::is_null),
        "{tree}"
    );
    let action = labelled(&all, "button", "Meeting", &tree);
    assert_eq!(action["description"], "Today\nReady\n5 min", "{tree}");
    assert!(action.get("selected").is_none_or(Value::is_null), "{tree}");
    assert!(
        action["on_action"]
            .as_array()
            .is_none_or(|actions| !actions.iter().any(|action| action == "Click")),
        "{tree}"
    );
    for (label, expanded, level, position, size) in
        [("Lectures", false, 2, 3, 7), ("Archive", true, 1, 1, 2)]
    {
        let row = labelled(&all, "treeItem", label, &tree);
        assert_eq!(row["expanded"], expanded, "{tree}");
        assert_eq!(row["level"], level, "{tree}");
        assert_eq!(row["position_in_set"], position, "{tree}");
        assert_eq!(row["size_of_set"], size, "{tree}");
    }
    assert_eq!(
        labelled(&all, "treeItem", "Lectures", &tree)["selected"],
        true
    );
    assert!(
        labelled(&all, "treeItem", "Archive", &tree)
            .get("selected")
            .is_none_or(Value::is_null),
        "{tree}"
    );
    assert_eq!(
        labelled(&all, "listItem", "Unselected item", &tree)["selected"],
        false,
        "{tree}"
    );
    assert_eq!(
        labelled(&all, "button", "Other destination", &tree)["selected"],
        false,
        "{tree}"
    );
    assert_eq!(
        labelled(&all, "button", "Other radio", &tree)["selected"],
        false,
        "{tree}"
    );
    let default = labelled(&all, "button", "Open", &tree);
    assert!(
        default["on_action"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|action| action == "Click")),
        "{default}"
    );
    for key in [
        "expanded",
        "level",
        "position_in_set",
        "size_of_set",
        "description",
        "selected",
    ] {
        assert!(default.get(key).is_none_or(Value::is_null), "{default}");
    }
    println!("{TEST}");
}

struct FieldSemantics {
    input: Entity<InputState>,
    area: Entity<TextareaState>,
    error: bool,
    feedback: bool,
    supporting: &'static str,
}

impl Render for FieldSemantics {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                TextField::new("secret", &self.input)
                    .label("API key")
                    .input_type(TextFieldInputType::Password)
                    .error(self.error)
                    .when(self.feedback, |field| {
                        field.supporting_text(self.supporting).max_length(20)
                    }),
            )
            .child(
                TextField::multi_line("notes", &self.area)
                    .label("Notes")
                    .supporting_text("Plain text")
                    .max_length(50),
            )
    }
}

#[gpui::test]
fn text_fields_publish_feedback_without_exposing_secrets(cx: &mut TestAppContext) {
    const TEST: &str = "text_fields_publish_feedback_without_exposing_secrets";
    if a11y_child(TEST) {
        return;
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);
        cx.set_reduce_motion(true);
    });
    let (view, cx) = cx.add_window_view(|window, cx| FieldSemantics {
        input: cx.new(|cx| InputState::new(window, cx).default_value("private-token")),
        area: cx.new(|cx| TextareaState::new(window, cx).default_value("\u{e9}\u{1f600}")),
        error: false,
        feedback: true,
        supporting: "Use a project key",
    });
    super::text::observe_live_text_nodes();
    let tree = painted_tree(cx);
    assert!(super::text::take_live_text_nodes().is_empty());
    let all = nodes(&tree);
    let secret = labelled(&all, "group", "API key", &tree);
    assert_eq!(secret["description"], "Use a project key\n13/20", "{tree}");
    labelled(&all, "label", "Use a project key", &tree);
    labelled(&all, "label", "13/20", &tree);
    let notes = labelled(&all, "group", "Notes", &tree);
    assert_eq!(notes["description"], "Plain text\n2/50", "{tree}");
    labelled(&all, "label", "Plain text", &tree);
    labelled(&all, "label", "2/50", &tree);
    let input = labelled(&all, "passwordInput", "API key", &tree);
    assert_eq!(
        input.get("description").and_then(Value::as_str),
        Some("Use a project key\n13/20"),
        "expected forwarded supporting text description: {tree}"
    );
    let input = labelled(&all, "multilineTextInput", "Notes", &tree);
    assert_eq!(
        input.get("description").and_then(Value::as_str),
        Some("Plain text\n2/50"),
        "expected forwarded supporting text description: {tree}"
    );
    assert!(!tree.contains("private-token"), "{tree}");

    view.update(cx, |view, cx| {
        view.error = true;
        cx.notify();
    });
    super::text::observe_live_text_nodes();
    let tree = painted_tree(cx);
    assert_live_text_nodes(
        &super::text::take_live_text_nodes(),
        gpui::Role::Alert,
        "Use a project key",
        gpui::accesskit::Live::Assertive,
    );
    let all = nodes(&tree);
    assert_eq!(
        labelled(&all, "alert", "Use a project key", &tree)["value"],
        "Use a project key",
        "{tree}"
    );
    assert!(!tree.contains("private-token"), "{tree}");

    view.update(cx, |view, cx| {
        view.supporting = "Key expired";
        cx.notify();
    });
    super::text::observe_live_text_nodes();
    let tree = painted_tree(cx);
    assert_live_text_nodes(
        &super::text::take_live_text_nodes(),
        gpui::Role::Alert,
        "Key expired",
        gpui::accesskit::Live::Assertive,
    );
    let all = nodes(&tree);
    assert_eq!(
        labelled(&all, "alert", "Key expired", &tree)["value"],
        "Key expired",
        "{tree}"
    );
    assert_eq!(
        labelled(&all, "group", "API key", &tree)["description"],
        "Key expired\n13/20",
        "{tree}"
    );
    assert!(!tree.contains("private-token"), "{tree}");

    view.update(cx, |view, cx| {
        view.error = false;
        cx.notify();
    });
    super::text::observe_live_text_nodes();
    let tree = painted_tree(cx);
    assert!(super::text::take_live_text_nodes().is_empty());
    let all = nodes(&tree);
    labelled(&all, "group", "API key", &tree);
    labelled(&all, "label", "Key expired", &tree);
    assert!(
        !all.iter().any(|node| node["role"]
            .as_str()
            .is_some_and(|role| role.eq_ignore_ascii_case("alert"))),
        "{tree}"
    );

    view.update(cx, |view, cx| {
        view.feedback = false;
        cx.notify();
    });
    super::text::observe_live_text_nodes();
    let tree = painted_tree(cx);
    assert!(super::text::take_live_text_nodes().is_empty());
    let all = nodes(&tree);
    assert!(
        !all.iter()
            .any(|node| node["description"] == "Key expired\n13/20"),
        "{tree}"
    );
    assert!(!all.iter().any(|node| node["label"] == "13/20"), "{tree}");
    labelled(&all, "passwordInput", "API key", &tree);
    assert!(!tree.contains("private-token"), "{tree}");
    println!("{TEST}");
}

fn assert_live_text_nodes(
    nodes: &[gpui::accesskit::Node],
    role: gpui::Role,
    value: &str,
    live: gpui::accesskit::Live,
) {
    assert!(!nodes.is_empty(), "no live text node was painted");
    for node in nodes {
        assert_eq!(node.role(), role, "{node:?}");
        assert_eq!(node.label(), Some(value), "{node:?}");
        assert_eq!(node.value(), Some(value), "{node:?}");
        assert_eq!(node.live(), Some(live), "{node:?}");
    }
}

struct LiveStatus(Option<&'static str>);
impl Render for LiveStatus {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .children(self.0.map(|message| status_text("status", message)))
    }
}

struct BorrowedButtonText<'a>(&'a str);
impl IntoElement for BorrowedButtonText<'_> {
    type Element = SharedString;

    fn into_element(self) -> Self::Element {
        self.0.to_owned().into()
    }
}

struct ButtonNames;

fn add_label<T: gpui::ParentElement>(button: T) -> T {
    button.child("Generic label")
}

impl Render for ButtonNames {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let borrowed = String::from("Borrowed text");
        div()
            .flex()
            .flex_col()
            .items_start()
            .size_full()
            .child(MaterialButton::new("literal").child("Skip setup"))
            .child(MaterialButton::new("owned").child(String::from("Save and continue")))
            .child(MaterialButton::new("shared").child(SharedString::new_static("Choose folder")))
            .child(MaterialButton::new("cow").child(std::borrow::Cow::Borrowed("Open settings")))
            .child(MaterialButton::new("borrowed-builder").child(BorrowedButtonText(&borrowed)))
            .child(
                MaterialButton::new("icon-and-text")
                    .icon(crate::icons::Symbol::CHECK)
                    .child("Confirm selection"),
            )
            .child(
                MaterialButton::new("explicit-first")
                    .accessibility_label("Save the current note")
                    .child("Save"),
            )
            .child(
                MaterialButton::new("explicit-last")
                    .child("Continue")
                    .accessibility_label("Continue to settings"),
            )
            .child(
                MaterialButton::new("blank")
                    .accessibility_label("Blank content action")
                    .child(""),
            )
            .child(
                MaterialButton::new("custom")
                    .child(div().child("Custom visual content"))
                    .accessibility_label("Custom content action"),
            )
            .child(
                MaterialButton::new("aria-first")
                    .aria_label("Save the current collection")
                    .child("Save collection"),
            )
            .child(
                MaterialButton::new("aria-last")
                    .child("Finish")
                    .aria_label("Finish setup"),
            )
            .child(MaterialButton::new("collection").children(["Save collection"]))
            .child(add_label(MaterialButton::new("generic")))
            .child(
                MaterialButton::new("mixed")
                    .child("Save")
                    .children(["and continue"]),
            )
    }
}

#[gpui::test]
fn buttons_derive_names_from_direct_text_and_preserve_explicit_labels(cx: &mut TestAppContext) {
    const TEST: &str = "buttons_derive_names_from_direct_text_and_preserve_explicit_labels";
    if a11y_child(TEST) {
        return;
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);
        cx.set_reduce_motion(true);
    });
    let (_, cx) = cx.add_window_view(|_, _| ButtonNames);
    let tree = painted_tree(cx);
    let all = nodes(&tree);
    let buttons: Vec<_> = all
        .iter()
        .filter(|node| {
            node["role"]
                .as_str()
                .is_some_and(|role| role.eq_ignore_ascii_case("button"))
        })
        .collect();
    assert_eq!(
        buttons.len(),
        15,
        "each component publishes one button: {tree}"
    );
    for name in [
        "Skip setup",
        "Save and continue",
        "Choose folder",
        "Open settings",
        "Borrowed text",
        "Confirm selection",
        "Save the current note",
        "Continue to settings",
        "Blank content action",
        "Custom content action",
        "Save the current collection",
        "Finish setup",
        "Save collection",
        "Generic label",
    ] {
        labelled(&all, "button", name, &tree);
    }
    assert_eq!(
        buttons
            .iter()
            .filter(|button| button["label"] == "Save and continue")
            .count(),
        2,
        "direct text and mixed insertion paths have the complete name: {tree}"
    );
    println!("{TEST}");
}

#[gpui::test]
fn status_text_publishes_polite_live_values_on_changes(cx: &mut TestAppContext) {
    const TEST: &str = "status_text_publishes_polite_live_values_on_changes";
    if a11y_child(TEST) {
        return;
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);
        cx.set_reduce_motion(true);
    });
    let (view, cx) = cx.add_window_view(|_, _| LiveStatus(None));
    super::text::observe_live_text_nodes();
    painted_tree(cx);
    assert!(super::text::take_live_text_nodes().is_empty());
    for (message, spoken) in [
        ("Waiting for scan", "Waiting for scan"),
        ("Confirm\u{2060} login", "Confirm login"),
    ] {
        view.update(cx, |view, cx| {
            view.0 = Some(message);
            cx.notify();
        });
        super::text::observe_live_text_nodes();
        let tree = painted_tree(cx);
        assert_live_text_nodes(
            &super::text::take_live_text_nodes(),
            gpui::Role::Status,
            spoken,
            gpui::accesskit::Live::Polite,
        );
        let all = nodes(&tree);
        assert_eq!(
            labelled(&all, "status", spoken, &tree)["value"],
            spoken,
            "{tree}"
        );
    }
    view.update(cx, |view, cx| {
        view.0 = None;
        cx.notify();
    });
    super::text::observe_live_text_nodes();
    let tree = painted_tree(cx);
    assert!(super::text::take_live_text_nodes().is_empty());
    assert!(
        !nodes(&tree).iter().any(|node| node["role"]
            .as_str()
            .is_some_and(|role| role.eq_ignore_ascii_case("status"))),
        "{tree}"
    );
    println!("{TEST}");
}

#[gpui::test]
fn overlays_rows_and_items_announce_names_and_states(cx: &mut TestAppContext) {
    if std::env::var("GPUI_A11Y_ACTIVE").as_deref() != Ok("1") {
        // Fast's switch is read once per process: assert in a child process
        // rather than racing other tests over the environment.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "components::a11y_tests::overlays_rows_and_items_announce_names_and_states",
                "--nocapture",
            ])
            .env("GPUI_A11Y_ACTIVE", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.contains(MARKER), "the child ran no assertions");
        return;
    }
    cx.update(|cx| {
        // The exposed dropdown's field draws through Kit's component theme.
        gpui_kit::init(cx);
        crate::init(cx);
        cx.set_reduce_motion(true);
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        window.set_rem_size(px(crate::tokens::typography::BASE_PX));
        Screen(cx.new(|cx| {
            let time = chrono::NaiveTime::from_hms_opt(7, 0, 0).unwrap();
            let mut picker = TimePickerState::new(time_labels(), time, window, cx);
            picker.toggle_mode(window, cx);
            picker
        }))
    });
    for _ in 0..3 {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    let tree = cx.update(|window, _| window.debug_a11y_tree_json().expect("accessibility on"));
    let all = nodes(&tree);

    // A list row announces its state and supporting text, not only its
    // headline.
    let row = labelled(&all, "button", "Wi-Fi", &tree);
    assert_eq!(row["selected"], true, "{row}");
    assert!(
        row["toggled"]
            .as_str()
            .is_some_and(|t| t.eq_ignore_ascii_case("true")),
        "{row}"
    );
    assert_eq!(row["description"], "Connected", "{row}");

    // A selectable card is a toggle button by default…
    let toggled = |node: &Value| {
        node["toggled"]
            .as_str()
            .is_some_and(|t| t.eq_ignore_ascii_case("true"))
    };
    let card = labelled(&all, "button", "Toggle card", &tree);
    assert!(toggled(card), "{card}");
    // …and in a card group a radio button with its place in the set.
    labelled(&all, "radioGroup", "Card choices", &tree);
    for (position, name, chosen) in [(1, "First choice", false), (2, "Second choice", true)] {
        let node = labelled(&all, "radioButton", name, &tree);
        assert_eq!(toggled(node), chosen, "{node}");
        assert_eq!(node["position_in_set"], position, "{node}");
        assert_eq!(node["size_of_set"], 2, "{node}");
    }

    // Caller-drawn options are Base's radios in a radio group, the
    // selection toggled, each with its place in the set.
    labelled(&all, "radioGroup", "Accent color", &tree);
    for (position, name) in [(1, "Red"), (2, "Green"), (3, "Blue")] {
        let node = labelled(&all, "radioButton", name, &tree);
        assert_eq!(toggled(node), position == 3, "{node}");
        assert_eq!(node["position_in_set"], position, "{node}");
        assert_eq!(node["size_of_set"], 3, "{node}");
    }

    // A drop target is a group named by its caller.
    labelled(&all, "group", "Upload files", &tree);

    // Overlays are named by the caller's headline or label.
    labelled(&all, "dialog", "Discard draft?", &tree);
    labelled(&all, "dialog", "Details", &tree);
    labelled(&all, "dialog", "Filters", &tree);
    // A dropdown that draws no label is still named by it, required mark included,
    // and announces its value.
    let language = all
        .iter()
        .find(|node| {
            node["role"]
                .as_str()
                .is_some_and(|r| r.eq_ignore_ascii_case("comboBox"))
                && node["label"]
                    .as_str()
                    .is_some_and(|l| l.starts_with("Language"))
        })
        .unwrap_or_else(|| panic!("no comboBox named Language in {tree}"));
    assert_ne!(
        language["label"], "Language",
        "the required mark: {language}"
    );
    assert_eq!(language["value"], "Chinese", "{language}");

    // A time picker is named for the mode it is in, here the input.
    labelled(&all, "dialog", "Enter time", &tree);

    // A dropdown with the caller's menu announces the value it shows.
    let model = labelled(&all, "comboBox", "Model", &tree);
    assert_eq!(model["value"], "Deep", "{model}");
    let actions = model["on_action"]
        .as_array()
        .unwrap_or_else(|| panic!("expected actions on {model}"));
    for expected in ["Click", "Expand", "Collapse"] {
        assert!(
            actions.iter().any(|action| action == expected),
            "missing {expected} action on {model}"
        );
    }

    // A clipped value's tooltip does not describe the field again: the field's
    // accessible value is already the whole text.
    let speech = labelled(&all, "comboBox", "Speech model", &tree);
    assert_eq!(speech["value"], CLIPPED_VALUE, "{speech}");
    assert!(
        !all.iter().any(|node| node["description"] == CLIPPED_VALUE),
        "{tree}"
    );

    // A status fact says in words what its color says.
    labelled(&all, "label", "Checked: passed", &tree);

    // A stepper is a list of its steps, each with its name, state and place in the flow.
    labelled(&all, "list", "Setup progress", &tree);
    for (position, step, state, current) in [
        (1, "Source", "Finished", false),
        (2, "Service", "Current", true),
        (3, "Account", "Upcoming", false),
    ] {
        let node = labelled(&all, "listItem", step, &tree);
        assert_eq!(node["position_in_set"], position, "{node}");
        assert_eq!(node["size_of_set"], 3, "{node}");
        assert_eq!(node["description"], state, "{node}");
        assert_eq!(node["selected"], current, "{node}");
    }

    // Carousel items are list items named by their label.
    let item = labelled(&all, "listItem", "Beach", &tree);
    assert_eq!(item["description"], "Summer", "{item}");

    // The labelled progress shorthands are named, with Base's value range.
    let copy = labelled(&all, "progressIndicator", "Copy", &tree);
    assert_eq!(copy["numeric_value"], 25.0, "{copy}");
    labelled(&all, "progressIndicator", "Index", &tree);
    // A hidden tooltip still describes its trigger.
    assert!(
        all.iter().any(|node| node["description"] == "Save a copy"),
        "{tree}"
    );
    println!("{MARKER}");
}

struct Sliders(Entity<SliderState>, Entity<SliderState>);
impl Render for Sliders {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(400.))
            .child(MaterialSlider::new(&self.0, "Volume"))
            .child(MaterialSlider::new(&self.1, "Price"))
    }
}

#[gpui::test]
fn sliders_report_a_usable_step_and_range_thumbs_their_bounds(cx: &mut TestAppContext) {
    const SLIDER_MARKER: &str = "material-a11y-slider-verified";
    if std::env::var("GPUI_A11Y_ACTIVE").as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "components::a11y_tests::sliders_report_a_usable_step_and_range_thumbs_their_bounds",
                "--nocapture",
            ])
            .env("GPUI_A11Y_ACTIVE", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            stdout.contains(SLIDER_MARKER),
            "the child ran no assertions"
        );
        return;
    }
    cx.update(|cx| {
        gpui_base::init(cx);
        cx.set_reduce_motion(true);
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        window.set_rem_size(px(crate::tokens::typography::BASE_PX));
        // Two continuous sliders (step 0): a single one and a range.
        Sliders(
            cx.new(|_| SliderState::new().step(0.).max(200.).default_value(50.)),
            cx.new(|_| {
                SliderState::new()
                    .step(0.)
                    .max(200.)
                    .default_value((40., 120.))
            }),
        )
    });
    for _ in 0..3 {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    let tree = cx.update(|window, _| window.debug_a11y_tree_json().expect("accessibility on"));
    let all = nodes(&tree);

    // A continuous slider reports the keyboard's step, a hundredth of its range, not 0.
    let volume = labelled(&all, "slider", "Volume", &tree);
    assert_eq!(volume["numeric_value_step"], 2.0, "{volume}");

    // A range is two sliders, its thumbs, in a named group that has no value or
    // actions of its own.
    let group = labelled(&all, "group", "Price", &tree);
    assert!(
        group.get("numeric_value").is_none_or(Value::is_null),
        "{group}"
    );
    let actions = group["on_action"].as_array().cloned().unwrap_or_default();
    assert!(
        !actions
            .iter()
            .any(|action| action == "Increment" || action == "Decrement"),
        "{group}"
    );

    // Each range thumb is bounded by the other.
    let price: Vec<&Value> = all
        .iter()
        .filter(|node| {
            node["label"] == "Price"
                && node["role"]
                    .as_str()
                    .is_some_and(|r| r.eq_ignore_ascii_case("slider"))
        })
        .collect();
    assert_eq!(price.len(), 2, "only the thumbs are sliders: {tree}");
    let start: Vec<&&Value> = price
        .iter()
        .filter(|node| node["numeric_value"] == 40.0)
        .collect();
    assert_eq!(start.len(), 1, "{tree}");
    assert_eq!(start[0]["min_numeric_value"], 0.0, "{}", start[0]);
    assert_eq!(start[0]["max_numeric_value"], 120.0, "{}", start[0]);
    let end = price
        .iter()
        .find(|node| node["numeric_value"] == 120.0 && node["min_numeric_value"] == 40.0)
        .unwrap_or_else(|| panic!("no end thumb bounded by the start in {tree}"));
    assert_eq!(end["max_numeric_value"], 200.0, "{end}");
    for node in price {
        assert_eq!(node["numeric_value_step"], 2.0, "{node}");
    }
    println!("{SLIDER_MARKER}");
}
