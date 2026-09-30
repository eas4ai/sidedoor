//! Retained application-update state, independent of plugin updates.
use crate::app::host::Host;
use gpui_kit::{Context, Task};
use services::updates::{Backend, Github, Prepared, Release};
use std::{rc::Rc, sync::Arc, time::Duration};

pub const VERSION: &str = env!("SIDEDOOR_BUILD_VERSION");
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Checking,
    Current,
    Available(Release),
    Downloading(Release),
    Ready(Release),
    Installing,
    Failed(String),
}

pub struct AppUpdates {
    pub state: State,
    pub automatic: bool,
    backend: Arc<dyn Backend>,
    host: Rc<dyn Host>,
    prepared: Option<Prepared>,
    operation: Option<Task<()>>,
    _timer: Option<Task<()>>,
    notified_version: Option<String>,
}

impl AppUpdates {
    #[cfg(test)]
    pub fn use_test_backend(&mut self, backend: Arc<dyn Backend>) {
        self.backend = backend;
    }
    pub fn new(automatic: bool, live: bool, host: Rc<dyn Host>, cx: &mut Context<Self>) -> Self {
        Self::with_backend(automatic, live, host, Arc::new(Github), cx)
    }

    pub fn with_backend(
        automatic: bool,
        live: bool,
        host: Rc<dyn Host>,
        backend: Arc<dyn Backend>,
        cx: &mut Context<Self>,
    ) -> Self {
        let timer = live.then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(10))
                    .await;
                loop {
                    if this
                        .update(cx, |this, cx| {
                            if this.automatic
                                && matches!(
                                    this.state,
                                    State::Idle
                                        | State::Current
                                        | State::Available(_)
                                        | State::Failed(_)
                                )
                            {
                                this.check(true, cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                    cx.background_executor().timer(INTERVAL).await;
                }
            })
        });
        Self {
            automatic,
            backend,
            host,
            state: State::Idle,
            prepared: None,
            operation: None,
            _timer: timer,
            notified_version: None,
        }
    }

    pub fn busy(&self) -> bool {
        matches!(
            self.state,
            State::Checking | State::Downloading(_) | State::Installing
        )
    }

    pub fn check(&mut self, automatic: bool, cx: &mut Context<Self>) {
        if self.busy() || matches!(self.state, State::Ready(_)) {
            return;
        }
        self.state = State::Checking;
        let backend = self.backend.clone();
        self.operation = Some(cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { backend.check(VERSION) }).await;
            this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(Some(release)) => {
                        if automatic && this.notified_version.as_deref() != Some(&release.version) {
                            this.host.notify("Sidedoor", "Sidedoor update available", &format!("Version {} is ready to download. Open Settings › General to update.", release.version));
                            this.notified_version = Some(release.version.clone());
                        }
                        State::Available(release)
                    }
                    Ok(None) => State::Current,
                    Err(error) => State::Failed(error),
                };
                cx.notify();
            }).ok();
        }));
        cx.notify();
    }

    pub fn download(&mut self, cx: &mut Context<Self>) {
        let State::Available(release) = &self.state else {
            return;
        };
        let release = release.clone();
        self.state = State::Downloading(release.clone());
        let backend = self.backend.clone();
        self.operation = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { backend.prepare(&release) })
                .await;
            this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(prepared) => {
                        let release = prepared.release.clone();
                        this.prepared = Some(prepared);
                        State::Ready(release)
                    }
                    Err(error) => State::Failed(error),
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn restart(&mut self, cx: &mut Context<Self>) {
        let Some(prepared) = self.prepared.take() else {
            return;
        };
        self.state = State::Installing;
        let backend = self.backend.clone();
        self.operation = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { backend.launch(prepared) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(()) => cx.quit(),
                Err(error) => {
                    this.state = State::Failed(error);
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn discard(&mut self, cx: &mut Context<Self>) {
        if let State::Ready(release) = &self.state {
            self.state = State::Available(release.clone());
            self.prepared = None;
            cx.notify();
        }
    }
}

/// Written only after real application initialization succeeds. Helpers keep
/// the previous portable installation until the new process reports its version.
pub fn acknowledge_startup() {
    if let Some(path) = std::env::var_os("SIDEDOOR_UPDATE_RECEIPT")
        && let Err(error) = std::fs::write(path, VERSION)
    {
        eprintln!("sidedoor: couldn't acknowledge the update: {error}");
    }
}
