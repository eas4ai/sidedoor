//! The status menu: a StatusNotifierItem tray icon, where the desktop shows
//! one (KDE Plasma, Cinnamon, Xfce, GNOME with the AppIndicator extension),
//! with the same items as the macOS menu bar extra.

use crate::{LoginItem, linux::system};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    OpenSettings,
    Reload,
    Quit,
}

/// Commands chosen in the tray, which runs on its own D-Bus thread, waiting
/// for the UI thread's pump.
type Queue = Arc<Mutex<Vec<MenuCommand>>>;

type Handler = Box<dyn Fn(MenuCommand)>;

thread_local! {
    static HANDLER: RefCell<Option<(Handler, Queue)>> = RefCell::new(None);
}

struct Tray {
    queue: Queue,
    login: bool,
}

impl Tray {
    fn send(&self, command: MenuCommand) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(command);
        }
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "sidedoor".into()
    }

    fn title(&self) -> String {
        "Sidedoor".into()
    }

    fn icon_name(&self) -> String {
        // The macOS menu bar uses the "sidebar.right" symbol.
        "sidebar-show-right-symbolic".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(MenuCommand::OpenSettings);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, StandardItem};
        vec![
            StandardItem {
                label: "Settings…".into(),
                activate: Box::new(|this: &mut Self| this.send(MenuCommand::OpenSettings)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Launch at Login".into(),
                checked: self.login,
                activate: Box::new(|this: &mut Self| {
                    let enabled = !this.login;
                    match system::set_launch_at_login(enabled) {
                        Ok(()) => this.login = enabled,
                        Err(error) => {
                            system::notify("Sidedoor", "Couldn't change launch at login", &error)
                        }
                    }
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Reload".into(),
                activate: Box::new(|this: &mut Self| this.send(MenuCommand::Reload)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit Sidedoor".into(),
                activate: Box::new(|this: &mut Self| this.send(MenuCommand::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct StatusMenu {
    _handle: Option<ksni::blocking::Handle<Tray>>,
}

impl StatusMenu {
    pub fn install(handler: impl Fn(MenuCommand) + 'static) -> Self {
        use ksni::blocking::TrayMethods as _;
        let queue = Queue::default();
        HANDLER.with(|slot| *slot.borrow_mut() = Some((Box::new(handler), queue.clone())));
        let tray = Tray {
            queue,
            login: system::login_item() == LoginItem::On,
        };
        // Desktops without a tray still reach Settings from the dock's menu.
        // A tray host may start later, such as an extension enabled after
        // login; the icon appears then.
        let handle = tray
            .assume_sni_available(true)
            .spawn()
            .map_err(|error| eprintln!("sidedoor: no status icon ({error})"))
            .ok();
        Self { _handle: handle }
    }
}

/// Runs menu commands chosen since the last call, from the native pump.
pub fn dispatch() {
    let commands: Vec<MenuCommand> = HANDLER.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|(_, queue)| {
                queue
                    .lock()
                    .ok()
                    .map(|mut queue| std::mem::take(&mut *queue))
            })
            .unwrap_or_default()
    });
    if commands.is_empty() {
        return;
    }
    HANDLER.with(|slot| {
        if let Some((handler, _)) = &*slot.borrow() {
            for command in commands {
                handler(command);
            }
        }
    });
}

impl Drop for StatusMenu {
    fn drop(&mut self) {
        if let Some(handle) = self._handle.take() {
            handle.shutdown();
        }
        let _ = HANDLER.try_with(|slot| {
            if let Ok(mut slot) = slot.try_borrow_mut() {
                drop(slot.take());
            }
        });
    }
}
