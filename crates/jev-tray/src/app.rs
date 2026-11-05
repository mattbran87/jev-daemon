//! The tray program: one tray icon that shows whether `jevd` runs (the
//! jev-tray spec). A worker thread checks every 5 seconds and sends each
//! result to the event loop, which changes the icon and tooltip only on a
//! change. The menu's first item runs `jev start` or `jev stop` (the Start
//! and Stop spec). The `jev-tray` binary of the `jev` package calls [`run`].

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Instant;

use crate::control::{self, Action, RUN_LIMIT};
use crate::icon::{self, Shade};
use crate::login::Login;
use crate::one_copy;
use crate::state::{self, Checker, State, CHECK_LIMIT};
use jev_proto::Paths;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// What reaches the event loop from other threads.
enum Message {
    /// The result of one check, with the bar's shade at that time.
    Checked { state: State, shade: Shade },
    /// A click on a menu item.
    Menu(MenuEvent),
    /// The end of `jev start` or `jev stop`.
    Ran {
        action: Action,
        result: Result<(), String>,
    },
}

/// The icon in the tray, and what it shows now.
struct Tray {
    icon: TrayIcon,
    /// The last state that a check gave.
    state: State,
    /// The state, shade, and size of the image on show.
    shown: (State, Shade, u32),
    /// The tooltip on show.
    tooltip: String,
}

impl Tray {
    /// Shows `state`, and changes the image or the tooltip only when it
    /// differs from what is on show.
    fn show(&mut self, state: State, shade: Shade, size: u32, failure: Option<&str>) {
        self.state = state;
        if (state, shade, size) != self.shown {
            if let Some(image) = image(state, shade, size) {
                if set_image(&self.icon, image).is_ok() {
                    self.shown = (state, shade, size);
                }
            }
        }
        let text = state::tooltip(state, failure);
        if text != self.tooltip && self.icon.set_tooltip(Some(&text)).is_ok() {
            self.tooltip = text;
        }
    }
}

/// Puts `image` in the tray. In tray-icon 0.24, `set_icon_with_as_template`
/// changes the image on macOS only: on Windows and Linux it ignores the image
/// and still returns `Ok`. So only macOS uses it, for the template flag.
fn set_image(tray: &TrayIcon, image: Icon) -> tray_icon::Result<()> {
    #[cfg(target_os = "macos")]
    return tray.set_icon_with_as_template(Some(image), icon::TEMPLATE);
    #[cfg(not(target_os = "macos"))]
    tray.set_icon(Some(image))
}

fn image(state: State, shade: Shade, size: u32) -> Option<Icon> {
    let image = icon::rgba(icon::png(state, shade, size)).ok()?;
    Icon::from_rgba(image.pixels, image.width, image.height).ok()
}

/// The icon size in pixels for the primary screen.
fn size_now(target: &EventLoopWindowTarget<Message>) -> u32 {
    let scale = target
        .primary_monitor()
        .map_or(1.0, |monitor| monitor.scale_factor());
    icon::size_for(icon::POINTS, scale)
}

/// Prints `message`, and adds it to the log in `log_dir` when there is one.
/// A failure to write the log is ignored: there is no other place to tell.
fn note(log_dir: Option<&Path>, message: &str) {
    eprintln!("jev-tray: {message}");
    if let Some(dir) = log_dir {
        let _ = crate::log::append(dir, message);
    }
}

/// Reports a start-up failure: the log line, and on Windows, where there is
/// no console, a message box.
fn report(log_dir: Option<&Path>, message: &str) {
    note(log_dir, message);
    show_error(message);
}

/// Reports a start-up failure and exits.
fn fail(log_dir: Option<&Path>, message: &str) -> ! {
    report(log_dir, message);
    std::process::exit(1);
}

#[cfg(windows)]
fn show_error(message: &str) {
    use crate::wide;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let (text, title) = (wide(message), wide("jev-tray"));
    // SAFETY: both strings end in a zero and live past the call; a null
    // owner window is allowed.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Elsewhere the message reaches the terminal or the session log.
#[cfg(not(windows))]
fn show_error(_message: &str) {}

/// Checks every 5 seconds, and at once on a message from `wake`, until the
/// event loop is gone.
fn watch(paths: Paths, proxy: EventLoopProxy<Message>, wake: Receiver<()>) {
    // One checker for the whole loop, so a check that hangs cannot make
    // each later check add one more thread.
    let mut checker = Checker::new();
    loop {
        let started = Instant::now();
        let message = Message::Checked {
            state: checker.check(&paths, CHECK_LIMIT),
            // Read at each check, so a Windows theme change shows.
            shade: icon::shade_now(),
        };
        if proxy.send_event(message).is_err() {
            return;
        }
        match wake.recv_timeout(CHECK_LIMIT.saturating_sub(started.elapsed())) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            // The event loop is gone.
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Runs `action` on its own thread, and sends the result to the event loop.
fn start_action(action: Action, proxy: &EventLoopProxy<Message>) {
    let sender = proxy.clone();
    let spawned = std::thread::Builder::new()
        .name("jev-tray-control".into())
        .spawn(move || {
            let result = std::env::current_exe()
                .map_err(|error| error.to_string())
                .and_then(|exe| control::run_action(action, &exe, RUN_LIMIT));
            let _ = sender.send_event(Message::Ran { action, result });
        });
    if let Err(error) = spawned {
        let result = Err(format!("cannot start a thread: {error}"));
        let _ = proxy.send_event(Message::Ran { action, result });
    }
}

/// Writes or deletes the login entry, so it matches the check item. The
/// item has already changed when the click arrives.
fn apply_login(item: &CheckMenuItem, login: &Result<Login, String>) -> Result<(), String> {
    let login = login.as_ref().map_err(Clone::clone)?;
    if item.is_checked() {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        login.write(&exe).map_err(|error| error.to_string())
    } else {
        login.delete().map_err(|error| error.to_string())
    }
}

/// Runs the tray until the user picks Quit, then exits the process. Returns
/// at once when another copy of jev-tray already runs. A start-up failure
/// goes to the log (and a message box on Windows), and the process exits
/// with code 1.
pub fn run() {
    let paths = Paths::from_env()
        .unwrap_or_else(|error| fail(None, &format!("cannot find the jev folders: {error}")));
    let log_dir = paths.state_dir.clone();
    let log = Some(log_dir.as_path());
    // Held until the process exits: `run` below never returns.
    let _lock = match one_copy::lock(&paths) {
        Ok(Some(lock)) => lock,
        // Another jev-tray runs.
        Ok(None) => return,
        Err(error) => fail(log, &format!("cannot take the one-copy lock: {error}")),
    };

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<Message>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        // No Dock icon.
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }

    let login = Login::for_this_user().map_err(|error| error.to_string());
    // Read once, at start; a failed read shows the item unticked.
    let login_on = match login.as_ref().map(Login::is_on) {
        Ok(Ok(on)) => on,
        Ok(Err(error)) => {
            note(
                log,
                &format!("cannot read the Start at login entry: {error}"),
            );
            false
        }
        Err(error) => {
            note(
                log,
                &format!("cannot find the Start at login entry: {error}"),
            );
            false
        }
    };
    // The state is off until the first check, so the label starts as Start.
    let control_item = MenuItem::new(Action::Start.label(), true, None);
    let login_item = CheckMenuItem::new("Start at login", true, login_on, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[&control_item, &login_item, &quit_item])
        .unwrap_or_else(|error| fail(log, &format!("cannot build the menu: {error}")));
    let mut menu = Some(menu);

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(Message::Menu(event));
    }));
    let proxy = event_loop.create_proxy();
    let (wake, woken) = mpsc::channel();
    std::thread::Builder::new()
        .name("jev-tray-watch".into())
        .spawn(move || watch(paths, proxy, woken))
        .unwrap_or_else(|error| fail(log, &format!("cannot start the check thread: {error}")));
    let proxy = event_loop.create_proxy();

    let mut tray: Option<Tray> = None;
    // True while `jev start` or `jev stop` runs.
    let mut running = false;
    // A failed Start or Stop, with the state at that time. It shows until
    // the state changes or the next click on the item.
    let mut failure: Option<(String, State)> = None;
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            // The tray must be made inside the running loop (GTK on Linux).
            Event::NewEvents(StartCause::Init) => {
                let (state, shade, size) = (State::Off, icon::shade_now(), size_now(target));
                let text = state::tooltip(state, None);
                let mut builder = TrayIconBuilder::new()
                    .with_tooltip(&text)
                    .with_icon_as_template(icon::TEMPLATE)
                    // Any click opens the menu, whatever the default.
                    .with_menu_on_left_click(true);
                if let Some(menu) = menu.take() {
                    builder = builder.with_menu(Box::new(menu));
                }
                if let Some(image) = image(state, shade, size) {
                    builder = builder.with_icon(image);
                }
                match builder.build() {
                    Ok(icon) => {
                        tray = Some(Tray {
                            icon,
                            state,
                            shown: (state, shade, size),
                            tooltip: text,
                        })
                    }
                    Err(error) => {
                        let message = format!("cannot show the tray icon: {error}");
                        report(Some(&log_dir), &message);
                        *control_flow = ControlFlow::ExitWithCode(1);
                    }
                }
            }
            Event::UserEvent(Message::Checked { state, shade }) => {
                if let Some(tray) = tray.as_mut() {
                    if failure.as_ref().is_some_and(|(_, at)| *at != state) {
                        failure = None;
                    }
                    // A new check ends the display of a login error.
                    let line = failure.as_ref().map(|(line, _)| line.as_str());
                    tray.show(state, shade, size_now(target), line);
                    let label = Action::for_state(state).label();
                    if control_item.text() != label {
                        control_item.set_text(label);
                    }
                }
            }
            Event::UserEvent(Message::Ran { action, result }) => {
                running = false;
                control_item.set_enabled(true);
                if let Err(error) = result {
                    let line = format!("{} failed: {error}", action.label());
                    note(Some(&log_dir), &line);
                    if let Some(tray) = tray.as_mut() {
                        let (_, shade, size) = tray.shown;
                        let state = tray.state;
                        tray.show(state, shade, size, Some(&line));
                        failure = Some((line, state));
                    }
                }
                // Check now, so the icon and the label follow at once.
                let _ = wake.send(());
            }
            Event::UserEvent(Message::Menu(event)) => {
                if event.id == *quit_item.id() {
                    // Drop the icon first, so Windows does not keep a ghost.
                    tray = None;
                    *control_flow = ControlFlow::Exit;
                } else if event.id == *control_item.id() {
                    if let (false, Some(tray)) = (running, tray.as_mut()) {
                        running = true;
                        control_item.set_enabled(false);
                        // The click ends the display of a failed Start or Stop.
                        failure = None;
                        let (state, (_, shade, size)) = (tray.state, tray.shown);
                        tray.show(state, shade, size, None);
                        start_action(Action::for_state(state), &proxy);
                    }
                } else if event.id == *login_item.id() {
                    if let Err(error) = apply_login(&login_item, &login) {
                        login_item.set_checked(!login_item.is_checked());
                        // A login error replaces a failed Start or Stop.
                        failure = None;
                        if let Some(tray) = tray.as_mut() {
                            let (_, shade, size) = tray.shown;
                            let state = tray.state;
                            let line = format!("Start at login failed: {error}");
                            tray.show(state, shade, size, Some(&line));
                        }
                    }
                }
            }
            _ => {}
        }
    });
}
