//! Notification Center banners, for plugins' `sidedoor.notify`.

use block2::{DynBlock, RcBlock};
use objc2::{
    AnyThread, define_class, msg_send,
    rc::Retained,
    runtime::{Bool, NSObject, ProtocolObject},
};
use objc2_foundation::{NSBundle, NSError, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationSound,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::{
    cell::OnceCell,
    sync::atomic::{AtomicU64, Ordering},
};

define_class!(
    // SAFETY: NSObject has no subclassing requirements and `Presenter` does
    // not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "SidedoorNotificationPresenter"]
    struct Presenter;

    unsafe impl NSObjectProtocol for Presenter {}

    unsafe impl UNUserNotificationCenterDelegate for Presenter {
        /// Shows banners even while Settings has the app in front.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }
    }
);

thread_local! {
    /// The center only holds its delegate weakly.
    static PRESENTER: OnceCell<Retained<Presenter>> = const { OnceCell::new() };
}

pub fn show(source: &str, title: &str, body: &str) {
    // Notification Center needs a bundled app; `cargo run` has none, and
    // asking for the center there throws.
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        eprintln!("sidedoor: notification from {source}: {title}: {body}");
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    PRESENTER.with(|presenter| {
        let presenter = presenter.get_or_init(|| {
            // SAFETY: NSObject's init on a freshly allocated subclass.
            unsafe { msg_send![Presenter::alloc(), init] }
        });
        center.setDelegate(Some(ProtocolObject::from_ref(&**presenter)));
    });

    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setSubtitle(&NSString::from_str(source));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = format!("plugin-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&id),
        &content,
        None,
    );

    // The system asks the user once; later requests answer straight away.
    let add = center.clone();
    let source = source.to_string();
    let authorized = RcBlock::new(move |granted: Bool, _: *mut NSError| {
        if granted.as_bool() {
            add.addNotificationRequest_withCompletionHandler(&request, None);
        } else {
            eprintln!("sidedoor: notifications are off for Sidedoor; {source} wasn't shown");
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &authorized,
    );
}
