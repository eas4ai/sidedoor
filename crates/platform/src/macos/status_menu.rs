//! The menu-bar item: the one place to reach the clone's settings,
//! launch at login, reload and quit, since the app has no Dock icon.

use crate::LoginItem;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::NSString;
use std::cell::OnceCell;

/// What a menu item asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    OpenSettings,
    Reload,
    Quit,
}

pub struct Ivars {
    handler: Box<dyn Fn(MenuCommand)>,
    login_item: OnceCell<Retained<NSMenuItem>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and `MenuTarget`
    // does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SidedoorMenuTarget"]
    #[ivars = Ivars]
    pub struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            (self.ivars().handler)(MenuCommand::OpenSettings);
        }

        #[unsafe(method(toggleLaunchAtLogin:))]
        fn toggle_launch_at_login(&self, _sender: Option<&AnyObject>) {
            let enable = matches!(crate::macos::login_item(), LoginItem::Off);
            if let Err(err) = crate::macos::set_launch_at_login(enable) {
                eprintln!("sidedoor: couldn't change launch at login: {err}");
            }
            self.refresh_login_item();
        }

        #[unsafe(method(reload:))]
        fn reload(&self, _sender: Option<&AnyObject>) {
            (self.ivars().handler)(MenuCommand::Reload);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            (self.ivars().handler)(MenuCommand::Quit);
        }
    }
);

impl MenuTarget {
    fn refresh_login_item(&self) {
        let Some(item) = self.ivars().login_item.get() else {
            return;
        };
        let state = crate::macos::login_item();
        item.setEnabled(state != LoginItem::Unavailable);
        item.setState(if state == LoginItem::On {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        let title = match state {
            LoginItem::NeedsApproval => "Launch at Login (Allow in System Settings)",
            LoginItem::Unavailable => "Launch at Login (App Bundle Only)",
            LoginItem::On | LoginItem::Off => "Launch at Login",
        };
        item.setTitle(&NSString::from_str(title));
    }
}

/// Keeps the status item on screen for as long as it is alive.
pub struct StatusMenu {
    _item: Retained<NSStatusItem>,
    _target: Retained<MenuTarget>,
}

impl StatusMenu {
    pub fn install(handler: impl Fn(MenuCommand) + 'static) -> Self {
        let mtm = MainThreadMarker::new().expect("the status item is created on the main thread");
        let target = MenuTarget::alloc(mtm).set_ivars(Ivars {
            handler: Box::new(handler),
            login_item: OnceCell::new(),
        });
        // SAFETY: NSObject's `init` is always valid on a fresh allocation.
        let target: Retained<MenuTarget> = unsafe { msg_send![super(target), init] };

        let menu = NSMenu::new(mtm);
        let add = |title: &str, action: Sel, key: &str| {
            // SAFETY: `action` is a method `MenuTarget` implements, and the
            // target outlives the menu (both live in `StatusMenu`).
            let item = unsafe {
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(title),
                    Some(action),
                    &NSString::from_str(key),
                );
                item.setTarget(Some(&target));
                item
            };
            menu.addItem(&item);
            item
        };
        add("Settings…", sel!(openSettings:), ",");
        let login = add("Launch at Login", sel!(toggleLaunchAtLogin:), "");
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add("Reload", sel!(reload:), "r");
        add("Quit Sidedoor", sel!(quit:), "q");
        let _ = target.ivars().login_item.set(login);
        target.refresh_login_item();

        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        if let Some(button) = item.button(mtm) {
            let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str("sidebar.right"),
                Some(&NSString::from_str("Sidedoor")),
            );
            if let Some(image) = image {
                image.setTemplate(true);
                button.setImage(Some(&image));
            }
        }
        item.setMenu(Some(&menu));
        Self {
            _item: item,
            _target: target,
        }
    }
}
