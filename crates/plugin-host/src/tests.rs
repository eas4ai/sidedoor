use super::*;
use crate::sdk::{install_sdk_copy, slug};
use serde_json::{Map, Value};
use std::{fs, path::PathBuf};

#[test]
fn sdk_copy_repairs_partial_installs_updates_owned_files_and_preserves_user_packages() {
    let dir = std::env::temp_dir().join(format!("sidedoor-sdk-copy-{}", std::process::id()));
    let source = dir.join("source");
    let destination = dir.join("plugin/node_modules/@sidedoor/sdk");
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("package.json"), r#"{"name":"@sidedoor/sdk"}"#).unwrap();
    fs::write(source.join("src/index.ts"), "export const version = 1;").unwrap();
    install_sdk_copy(&source, &destination).unwrap();
    assert_eq!(
        fs::read_to_string(destination.join("src/index.ts")).unwrap(),
        "export const version = 1;"
    );
    fs::write(source.join("src/index.ts"), "export const version = 2;").unwrap();
    fs::remove_file(destination.join("package.json")).unwrap();
    install_sdk_copy(&source, &destination).unwrap();
    assert!(destination.join("package.json").is_file());
    assert_eq!(
        fs::read_to_string(destination.join("src/index.ts")).unwrap(),
        "export const version = 2;"
    );
    fs::remove_file(destination.join(".sidedoor-sdk")).unwrap();
    fs::write(destination.join("src/index.ts"), "user-installed SDK").unwrap();
    install_sdk_copy(&source, &destination).unwrap();
    assert_eq!(
        fs::read_to_string(destination.join("src/index.ts")).unwrap(),
        "user-installed SDK"
    );
    fs::remove_dir_all(dir).unwrap();
}

fn element(kind: &str, children: Vec<Node>) -> Node {
    Node::Element {
        kind: kind.into(),
        props: Map::new(),
        children,
    }
}

#[test]
fn reads_trees_and_messages() {
    let json = r#"{"type":"render","surface":"card","plugin":"timer","tree":[
            {"t":"Card","p":{"title":"Timer"},"c":[
                {"t":"div","p":{"flex":true,"on_click":{"$h":"card:Widget/#on_click"}},"c":["25:00"]}
            ]}
        ]}"#;
    let PluginMessage::Render { surface, tree } = serde_json::from_str(json).unwrap() else {
        panic!("expected a render");
    };
    assert_eq!(surface, "card");
    let Node::Element { kind, children, .. } = &tree[0] else {
        panic!("expected an element");
    };
    assert_eq!(kind, "Card");
    let Node::Element {
        props, children, ..
    } = &children[0]
    else {
        panic!("expected an element");
    };
    assert_eq!(props["flex"], Value::Bool(true));
    assert_eq!(children, &vec![Node::Text("25:00".into())]);
    assert_eq!(
        serde_json::from_str::<PluginMessage>(r#"{"type":"exited","plugin":"timer"}"#).unwrap(),
        PluginMessage::Exited
    );
}

#[test]
fn host_messages_are_json_the_sdk_reads() {
    let event = HostMessage::Event {
        handler: "card:Widget/#on_click".into(),
        value: Value::Bool(true),
    };
    assert_eq!(
        serde_json::to_string(&event).unwrap(),
        r#"{"type":"event","handler":"card:Widget/#on_click","value":true}"#
    );
    assert_eq!(
        serde_json::to_string(&HostMessage::Card { open: true }).unwrap(),
        r#"{"type":"card","open":true}"#
    );
    assert_eq!(
        serde_json::to_string(&HostMessage::Click).unwrap(),
        r#"{"type":"click"}"#
    );
    assert_eq!(
        serde_json::to_string(&HostMessage::Action {
            key: "reset".into()
        })
        .unwrap(),
        r#"{"type":"action","key":"reset"}"#
    );
}

#[test]
fn patches_edit_the_tree_in_place() {
    let mut tree = vec![element(
        "div",
        vec![Node::Text("a".into()), element("div", vec![])],
    )];
    let patches: Vec<Patch> = serde_json::from_str(
        r#"[{"op":"replace","path":[0,0],"node":"b"},
                {"op":"props","path":[0,1],"props":{"w":20}}]"#,
    )
    .unwrap();
    assert!(apply_patches(&mut tree, patches));
    let Node::Element { children, .. } = &tree[0] else {
        panic!("expected an element");
    };
    assert_eq!(children[0], Node::Text("b".into()));
    let Node::Element { props, .. } = &children[1] else {
        panic!("expected an element");
    };
    assert_eq!(props["w"], Value::from(20));

    let wrong: Vec<Patch> =
        serde_json::from_str(r#"[{"op":"props","path":[0,0],"props":{}}]"#).unwrap();
    assert!(!apply_patches(&mut tree, wrong), "text has no props");
    let missing: Vec<Patch> =
        serde_json::from_str(r#"[{"op":"replace","path":[3],"node":"x"}]"#).unwrap();
    assert!(!apply_patches(&mut tree, missing));
}

#[test]
fn plugins_are_found_by_their_definition_without_running_them() {
    let dir = std::env::temp_dir().join(format!("sidedoor-plugin-{}", std::process::id()));
    let plugin = dir.join("pomodoro");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join("index.tsx"),
        r#"import { definePlugin } from "@sidedoor/sdk";
               const label = { name: "not this one" };
               export default definePlugin({
                 icon: 'timer',
                 name: "Pomodoro",
                 card: () => <div>{label.name}</div>,
               });"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("unnamed")).unwrap();
    fs::write(
        dir.join("unnamed/index.ts"),
        "export default definePlugin({ card })",
    )
    .unwrap();
    fs::create_dir_all(dir.join("not-a-plugin")).unwrap();
    fs::write(dir.join("not-a-plugin/index.ts"), "console.log(1)").unwrap();

    let found = discover(&dir);
    fs::remove_dir_all(&dir).ok();
    assert_eq!(found.len(), 2);
    let pomodoro = found.iter().find(|m| m.id == "pomodoro").unwrap();
    assert_eq!(pomodoro.name, "Pomodoro");
    assert_eq!(pomodoro.icon_path(), "icons/timer.svg");
    assert_eq!(pomodoro.main, PathBuf::from("index.tsx"));
    let unnamed = found.iter().find(|m| m.id == "unnamed").unwrap();
    assert_eq!(
        (unnamed.name.as_str(), unnamed.icon.as_str()),
        ("unnamed", "puzzle")
    );
}

#[test]
fn the_running_plugin_describes_itself() {
    let json = r#"{"type":"manifest","plugin":"pomodoro","name":"Pomodoro","icon":"timer",
            "width":9000,"height":null,"clickable":true,
            "actions":[{"key":"skip","title":"Skip Break"}],
            "windows":[{"key":"history","title":"History","width":50}],
            "settings":[{"key":"sound","title":"Sound","type":"toggle"},
                        {"key":"mode","title":"Mode","type":"choice","options":["focus","break"]}]}"#;
    let PluginMessage::Manifest(described) = serde_json::from_str(json).unwrap() else {
        panic!("expected a manifest");
    };
    let mut manifest = Manifest {
        id: "pomodoro".into(),
        name: "pomodoro".into(),
        icon: "puzzle".into(),
        width: 280.0,
        height: None,
        settings: Vec::new(),
        clickable: false,
        actions: Vec::new(),
        windows: Vec::new(),
        data: Vec::new(),
        dir: PathBuf::from("/plugins/pomodoro"),
        main: PathBuf::from("index.tsx"),
    };
    manifest.update(described);
    assert_eq!(manifest.name, "Pomodoro");
    assert_eq!(manifest.width, 480.0);
    assert_eq!(manifest.height, None);
    assert!(manifest.clickable);
    assert_eq!(manifest.actions[0].title, "Skip Break");
    assert_eq!(
        (manifest.windows[0].width, manifest.windows[0].height),
        (280.0, 360.0)
    );

    let saved = Map::from_iter([("mode".to_string(), Value::from("break"))]);
    let values = manifest.settings_with(Some(&saved));
    assert_eq!(values["sound"], Value::from(false));
    assert_eq!(values["mode"], Value::from("break"));
    assert_eq!(manifest.settings_with(None)["mode"], Value::from("focus"));
}

#[test]
fn new_plugins_get_a_folder_and_a_working_template() {
    let dir = std::env::temp_dir().join(format!("sidedoor-new-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let first = create(&dir, "My Widget!").unwrap();
    let second = create(&dir, "My Widget!").unwrap();
    let source = fs::read_to_string(first.dir.join("index.tsx")).unwrap();
    let has_package = first.dir.join("package.json").exists();
    fs::remove_dir_all(&dir).ok();
    assert_eq!(first.id, "my-widget");
    assert_eq!(second.id, "my-widget-2");
    assert_eq!(first.name, "My Widget!");
    assert_eq!(first.icon, "sparkles");
    assert!(source.contains(r#"name: "My Widget!","#));
    assert!(source.contains(r#"<Card title={"My Widget!"}"#));
    assert!(!has_package, "everything lives in definePlugin");
    assert_eq!(slug("  ..  "), "widget");
}

#[test]
fn preparing_a_plugin_keeps_sdk_imports_at_the_scoped_package_path() {
    let dir = std::env::temp_dir().join(format!("sidedoor-sdk-path-{}", std::process::id()));
    let sdk = dir.join("source-sdk");
    let plugin = dir.join("plugin");
    fs::create_dir_all(sdk.join("src")).unwrap();
    fs::write(sdk.join("package.json"), r#"{"name":"@sidedoor/sdk"}"#).unwrap();
    fs::write(sdk.join("src/index.ts"), "export const ready = true;").unwrap();
    crate::sdk::prepare(&plugin, &sdk).unwrap();
    assert_eq!(
        fs::read_to_string(plugin.join("node_modules/@sidedoor/sdk/src/index.ts")).unwrap(),
        "export const ready = true;"
    );
    assert!(plugin.join("tsconfig.json").is_file());
    fs::remove_dir_all(dir).unwrap();
}
