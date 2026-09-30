use crate::manifest::Described;
use futures::channel::mpsc::UnboundedReceiver;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
// MARK: Protocol

/// A rendered node: text, or an element with props and children.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Node {
    Text(String),
    Element {
        #[serde(rename = "t")]
        kind: String,
        #[serde(rename = "p", default)]
        props: Map<String, Value>,
        #[serde(rename = "c", default)]
        children: Vec<Node>,
    },
}

/// A change to a rendered surface. `path` indexes into children, starting
/// with the root.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Patch {
    Replace {
        path: Vec<usize>,
        node: Node,
    },
    Props {
        path: Vec<usize>,
        props: Map<String, Value>,
    },
}

fn node_at<'a>(tree: &'a mut [Node], path: &[usize]) -> Option<&'a mut Node> {
    let (first, rest) = path.split_first()?;
    let mut node = tree.get_mut(*first)?;
    for index in rest {
        match node {
            Node::Element { children, .. } => node = children.get_mut(*index)?,
            Node::Text(_) => return None,
        }
    }
    Some(node)
}

/// Applies `patches` in order. `false` if one doesn't fit the tree, which
/// means the host and the plugin disagree and the surface should be resent.
pub fn apply_patches(tree: &mut [Node], patches: Vec<Patch>) -> bool {
    for patch in patches {
        match patch {
            Patch::Replace { path, node } => match node_at(tree, &path) {
                Some(target) => *target = node,
                None => return false,
            },
            Patch::Props { path, props } => match node_at(tree, &path) {
                Some(Node::Element { props: target, .. }) => *target = props,
                _ => return false,
            },
        }
    }
    true
}

/// What a plugin sends, and what the supervisor says about it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginMessage {
    /// Sent first: what `definePlugin` declares.
    Manifest(Described),
    Render {
        surface: String,
        tree: Vec<Node>,
    },
    Patch {
        surface: String,
        patches: Vec<Patch>,
    },
    OpenUrl {
        url: String,
    },
    OpenPath {
        path: String,
    },
    Copy {
        text: String,
    },
    Notify {
        title: String,
        #[serde(default)]
        body: String,
    },
    OpenWindow {
        key: String,
    },
    CloseWindow {
        key: String,
    },
    /// A `console.log` line.
    Log {
        line: String,
    },
    Error {
        message: String,
    },
    /// The plugin's worker stopped.
    Exited,
}

/// What the host sends a plugin.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    Event {
        handler: String,
        value: Value,
    },
    /// Whether the plugin's card is showing.
    Card {
        open: bool,
    },
    /// The dock tile was clicked, or the item's shortcut pressed.
    Click,
    /// A command from the item's context menu.
    Action {
        key: String,
    },
    /// One of the plugin's windows opened or closed.
    Window {
        key: String,
        open: bool,
    },
    Settings {
        values: Map<String, Value>,
    },
    /// Send every surface whole again.
    Resync,
}

/// The host's side of a running plugin.
pub trait PluginLink {
    fn send(&mut self, message: &HostMessage);
}

/// A running plugin: messages to it, and what it sends back.
pub struct Connection {
    pub link: Box<dyn PluginLink>,
    pub incoming: UnboundedReceiver<PluginMessage>,
}
