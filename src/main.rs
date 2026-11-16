#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod autostart;
mod config;
mod monitor;
mod resolver;
mod watcher;

use crate::{
    config::AppConfig,
    monitor::{MonitorEvent, WorkerEnvelope},
};
use iced::{
    Alignment, Color, Element, Fill, Length, Size, Subscription, Task, Theme, time,
    widget::{button, checkbox, column, container, row, svg, text},
    window,
};
use std::{
    env,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem},
};

const POLL_UI_MS: u64 = 200;
const TRAY_OPEN: &str = "open";
const TRAY_QUIT: &str = "quit";

fn main() -> iced::Result {
    iced::daemon(boot, update, view)
        .title("WoWs Replay Watcher")
        .subscription(subscription)
        .theme(app_theme)
        .run()
}

fn app_theme(_: &App, _: window::Id) -> Theme {
    Theme::custom(
        "WoWs Monitor",
        iced::theme::Palette {
            background: Color::from_rgb8(17, 22, 29),
            text: Color::from_rgb8(239, 243, 247),
            primary: Color::from_rgb8(241, 90, 34),
            success: Color::from_rgb8(76, 232, 170),
            warning: Color::from_rgb8(255, 194, 64),
            danger: Color::from_rgb8(227, 72, 80),
        },
    )
}

struct App {
    config: AppConfig,
    active_config: AppConfig,
    setup_mode: bool,
    status: String,
    status_is_error: bool,
    resolved_replay_path: Option<PathBuf>,
    resolved_region: Option<String>,
    last_copied_path: Option<PathBuf>,
    autostart_enabled: bool,
    autostart_error: Option<String>,
    tray_error: Option<String>,
    tray: Option<TrayHandle>,
    tray_initialized: bool,
    window_id: Option<window::Id>,
    minimize_on_open: bool,
    sender: Sender<WorkerEnvelope>,
    receiver: Receiver<WorkerEnvelope>,
    generation: u64,
    stop_worker: Option<Arc<AtomicBool>>,
}

struct TrayHandle {
    _icon: TrayIcon,
}

#[derive(Clone, Copy)]
enum TrayAction {
    Open,
    Quit,
}

#[derive(Debug, Clone)]
enum Message {
    PickGameFolder,
    PickTargetFolder,
    GameFolderPicked(Option<PathBuf>),
    TargetFolderPicked(Option<PathBuf>),
    StartMonitoring,
    CancelReconfigure,
    Reconfigure,
    HideWindow,
    SetAutostart(bool),
    Pulse,
    WindowClosed(window::Id),
    WindowOpened(window::Id),
    WindowCloseRequested(window::Id),
}

fn boot() -> (App, Task<Message>) {
    let (sender, receiver) = mpsc::channel();
    let (config, config_warning) = config::load();
    let is_configured = valid_configuration(&config);
    let minimize_on_open = env::args().any(|argument| argument == "--minimized");
    let mut app = App {
        config: config.clone(),
        active_config: config,
        setup_mode: !is_configured,
        status: config_warning.unwrap_or_else(|| {
            if is_configured {
                "Starting the replay watcher…".to_owned()
            } else {
                "Choose the World of Warships game folder and a destination folder.".to_owned()
            }
        }),
        status_is_error: false,
        resolved_replay_path: None,
        resolved_region: None,
        last_copied_path: None,
        autostart_enabled: autostart::is_enabled(),
        autostart_error: None,
        tray_error: None,
        tray: None,
        tray_initialized: false,
        window_id: None,
        minimize_on_open,
        sender,
        receiver,
        generation: 0,
        stop_worker: None,
    };

    if is_configured {
        app.start_worker();
    }

    let (id, task) = window::open(window_settings());
    app.window_id = Some(id);
    (app, task.map(Message::WindowOpened))
}

fn window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(620.0, 540.0),
        min_size: Some(Size::new(560.0, 480.0)),
        icon: app_icon(),
        exit_on_close_request: false,
        ..window::Settings::default()
    }
}

fn app_icon() -> Option<window::Icon> {
    let image = image::load_from_memory(include_bytes!("../assets/wows-monitor-icon.png"))
        .ok()?
        .to_rgba8();
    window::icon::from_rgba(image.into_raw(), 512, 512).ok()
}

fn valid_configuration(config: &AppConfig) -> bool {
    config
        .game_folder
        .as_ref()
        .is_some_and(|path| path.is_dir())
        && config
            .target_folder
            .as_ref()
            .is_some_and(|path| path.is_dir())
}

fn subscription(_app: &App) -> Subscription<Message> {
    Subscription::batch([
        time::every(Duration::from_millis(POLL_UI_MS)).map(|_| Message::Pulse),
        window::close_requests().map(Message::WindowCloseRequested),
        window::close_events().map(Message::WindowClosed),
    ])
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::PickGameFolder => Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title("Select the World of Warships game folder")
                    .pick_folder()
                    .await
                    .map(|folder| folder.path().to_path_buf())
            },
            Message::GameFolderPicked,
        ),
        Message::PickTargetFolder => Task::perform(
            async {
                rfd::AsyncFileDialog::new()
                    .set_title("Select the folder for copied tempArenaInfo.json files")
                    .pick_folder()
                    .await
                    .map(|folder| folder.path().to_path_buf())
            },
            Message::TargetFolderPicked,
        ),
        Message::GameFolderPicked(path) => {
            if let Some(path) = path {
                app.config.game_folder = Some(path);
                app.status = "Game folder selected.".to_owned();
                app.status_is_error = false;
            }
            Task::none()
        }
        Message::TargetFolderPicked(path) => {
            if let Some(path) = path {
                app.config.target_folder = Some(path);
                app.status = "Destination folder selected.".to_owned();
                app.status_is_error = false;
            }
            Task::none()
        }
        Message::StartMonitoring => {
            if !valid_configuration(&app.config) {
                app.status = "Select two existing folders before starting.".to_owned();
                app.status_is_error = true;
                return Task::none();
            }
            if let Err(error) = config::save(&app.config) {
                app.status = format!("Could not save settings: {error}");
                app.status_is_error = true;
                return Task::none();
            }

            app.active_config = app.config.clone();
            app.setup_mode = false;
            app.resolved_replay_path = None;
            app.resolved_region = None;
            app.last_copied_path = None;
            app.status = "Starting the replay watcher…".to_owned();
            app.status_is_error = false;
            app.start_worker();
            Task::none()
        }
        Message::CancelReconfigure => {
            if valid_configuration(&app.active_config) {
                app.config = app.active_config.clone();
                app.setup_mode = false;
            }
            Task::none()
        }
        Message::Reconfigure => {
            app.config = app.active_config.clone();
            app.setup_mode = true;
            open_window(app)
        }
        Message::HideWindow => close_window(app),
        Message::SetAutostart(enabled) => {
            match autostart::set_enabled(enabled) {
                Ok(()) => {
                    app.autostart_enabled = enabled;
                    app.autostart_error = None;
                }
                Err(error) => {
                    app.autostart_error = Some(error);
                }
            }
            Task::none()
        }
        Message::Pulse => {
            while let Ok(envelope) = app.receiver.try_recv() {
                if envelope.generation != app.generation {
                    continue;
                }
                match envelope.event {
                    MonitorEvent::Status(status) => {
                        app.status = status;
                        app.status_is_error = false;
                    }
                    MonitorEvent::Resolved { path, region } => {
                        app.status = "Monitoring the resolved replay folder.".to_owned();
                        app.status_is_error = false;
                        app.resolved_replay_path = Some(path);
                        app.resolved_region = Some(region);
                    }
                    MonitorEvent::Copied { destination } => {
                        app.status =
                            format!("Copied tempArenaInfo.json to {}.", destination.display());
                        app.status_is_error = false;
                        app.last_copied_path = Some(destination);
                    }
                    MonitorEvent::Error(error) => {
                        app.status = error;
                        app.status_is_error = true;
                    }
                }
            }

            let actions = app
                .tray
                .as_ref()
                .map(|_| {
                    let mut actions = Vec::new();
                    while let Ok(event) = MenuEvent::receiver().try_recv() {
                        match event.id.as_ref() {
                            TRAY_OPEN => actions.push(TrayAction::Open),
                            TRAY_QUIT => actions.push(TrayAction::Quit),
                            _ => {}
                        }
                    }
                    actions
                })
                .unwrap_or_default();
            let tasks = actions.into_iter().map(|action| match action {
                TrayAction::Open => open_window(app),
                TrayAction::Quit => {
                    app.stop_worker();
                    iced::exit()
                }
            });
            Task::batch(tasks)
        }
        Message::WindowClosed(id) => {
            if app.window_id == Some(id) {
                app.window_id = None;
            }
            Task::none()
        }
        Message::WindowCloseRequested(id) => {
            if app.tray.is_some() {
                window::close(id)
            } else {
                window::minimize(id, true)
            }
        }
        Message::WindowOpened(id) => {
            app.window_id = Some(id);
            if !app.tray_initialized {
                app.tray_initialized = true;
                match TrayHandle::new() {
                    Ok(tray) => app.tray = Some(tray),
                    Err(error) => app.tray_error = Some(error),
                }
            }
            if app.minimize_on_open {
                app.minimize_on_open = false;
                window::minimize(id, true)
            } else {
                Task::none()
            }
        }
    }
}

fn open_window(app: &mut App) -> Task<Message> {
    if let Some(id) = app.window_id {
        Task::batch([window::minimize(id, false), window::gain_focus(id)])
    } else {
        let (id, task) = window::open(window_settings());
        app.window_id = Some(id);
        task.map(Message::WindowOpened)
    }
}

fn close_window(app: &mut App) -> Task<Message> {
    match app.window_id {
        Some(id) if app.tray.is_some() => window::close(id),
        Some(id) => window::minimize(id, true),
        None => Task::none(),
    }
}

impl App {
    fn start_worker(&mut self) {
        self.stop_worker();
        let (Some(game_folder), Some(target_folder)) = (
            self.active_config.game_folder.clone(),
            self.active_config.target_folder.clone(),
        ) else {
            return;
        };

        self.generation += 1;
        let stop = Arc::new(AtomicBool::new(false));
        monitor::start(
            self.generation,
            game_folder,
            target_folder,
            self.sender.clone(),
            stop.clone(),
        );
        self.stop_worker = Some(stop);
    }

    fn stop_worker(&mut self) {
        if let Some(stop) = self.stop_worker.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

fn view(app: &App, _window: window::Id) -> Element<'_, Message> {
    if app.setup_mode {
        setup_view(app)
    } else {
        monitor_view(app)
    }
}

fn setup_view(app: &App) -> Element<'_, Message> {
    let can_start = valid_configuration(&app.config);
    let mut actions = row![
        checkbox(app.autostart_enabled)
            .label("Start when I log in")
            .on_toggle(Message::SetAutostart)
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    if can_start {
        actions = actions.push(button("Start monitoring").on_press(Message::StartMonitoring));
    } else {
        actions = actions.push(button("Start monitoring"));
    }
    if valid_configuration(&app.active_config) {
        actions = actions.push(
            button("Cancel")
                .style(button::secondary)
                .on_press(Message::CancelReconfigure),
        );
    }

    let mut content = column![
        brand_header(
            "Set up monitoring",
            "Choose the game installation and copy destination."
        ),
        container(
            column![
                folder_picker(
                    "Game folder",
                    app.config.game_folder.as_deref(),
                    if app.config.game_folder.is_some() {
                        "Change"
                    } else {
                        "Choose"
                    },
                    Message::PickGameFolder
                ),
                folder_picker(
                    "Destination folder",
                    app.config.target_folder.as_deref(),
                    if app.config.target_folder.is_some() {
                        "Change"
                    } else {
                        "Choose"
                    },
                    Message::PickTargetFolder
                ),
            ]
            .spacing(12)
        )
        .padding(16)
        .style(container::bordered_box),
        actions,
        status_panel(app),
    ]
    .spacing(16)
    .padding(24);
    if let Some(error) = &app.autostart_error {
        content = content.push(text(format!("Startup setting failed: {error}")).size(12));
    }
    if let Some(error) = &app.tray_error {
        content = content.push(text(format!("Tray icon unavailable: {error}")).size(12));
    }

    container(content)
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .into()
}

fn monitor_view(app: &App) -> Element<'_, Message> {
    let tray_message = if app.tray_error.is_some() {
        "System tray unavailable. Closing this window will minimize the app."
    } else {
        "Monitoring in the background. Close this window to leave it in the tray."
    };
    let close_label = if app.tray_error.is_some() {
        "Minimize"
    } else {
        "Close to tray"
    };

    let content = column![
        brand_header("Replay watcher", tray_message),
        status_panel(app),
        container(
            column![
                text("RESOLVED REPLAY PATH")
                    .size(11)
                    .color(Color::from_rgb8(165, 177, 191)),
                text(path_text(app.resolved_replay_path.as_deref()))
                    .size(14)
                    .width(Fill),
            ]
            .spacing(8)
        )
        .padding(16)
        .style(container::bordered_box),
        row![
            detail_card("GAME FOLDER", app.config.game_folder.as_deref()),
            detail_card("COPY DESTINATION", app.config.target_folder.as_deref()),
        ]
        .spacing(12),
        checkbox(app.autostart_enabled)
            .label("Start when I log in")
            .on_toggle(Message::SetAutostart),
        row![
            button("Reconfigure")
                .style(button::secondary)
                .on_press(Message::Reconfigure),
            button(close_label).on_press(Message::HideWindow),
        ]
        .spacing(12),
    ]
    .spacing(14)
    .padding(24);

    let mut content = content;
    if let Some(error) = &app.autostart_error {
        content = content.push(text(format!("Startup setting failed: {error}")).size(12));
    }
    if let Some(error) = &app.tray_error {
        content = content.push(text(format!("Tray icon unavailable: {error}")).size(12));
    }

    container(content)
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .into()
}

fn brand_header<'a>(title: &'a str, subtitle: &'a str) -> iced::widget::Column<'a, Message> {
    column![
        row![
            svg(iced::widget::svg::Handle::from_memory(
                include_bytes!("../assets/wows-monitor-logo.svg").as_slice()
            ))
            .width(Length::Fixed(27.0))
            .height(Length::Fixed(36.0)),
            column![
                text(title).size(23),
                text(subtitle)
                    .size(13)
                    .color(Color::from_rgb8(165, 177, 191)),
            ]
            .spacing(3),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
    ]
}

fn folder_picker<'a>(
    title: &'a str,
    path: Option<&Path>,
    button_label: &'a str,
    message: Message,
) -> iced::widget::Row<'a, Message> {
    row![
        column![
            text(title).size(14),
            text(path_text(path))
                .size(12)
                .color(Color::from_rgb8(165, 177, 191))
                .width(Fill),
        ]
        .spacing(5)
        .width(Fill),
        button(button_label)
            .style(button::secondary)
            .on_press(message),
    ]
    .spacing(14)
    .align_y(Alignment::Center)
}

fn detail_card<'a>(label: &'a str, path: Option<&Path>) -> Element<'a, Message> {
    container(
        column![
            text(label).size(10).color(Color::from_rgb8(165, 177, 191)),
            text(path_text(path)).size(12).width(Fill),
        ]
        .spacing(7),
    )
    .padding(14)
    .width(Fill)
    .style(container::bordered_box)
    .into()
}

fn status_panel(app: &App) -> Element<'_, Message> {
    let (state, dot_color) = if app.status_is_error {
        ("NEEDS ATTENTION", Color::from_rgb8(227, 72, 80))
    } else if app.resolved_replay_path.is_some() {
        ("MONITORING", Color::from_rgb8(76, 232, 170))
    } else {
        ("STARTING", Color::from_rgb8(255, 194, 64))
    };
    let region = app.resolved_region.as_deref().unwrap_or("—");
    container(
        column![
            row![
                text("●").size(12).color(dot_color),
                text(state).size(11),
                text(format!("REGION {region}"))
                    .size(11)
                    .color(Color::from_rgb8(165, 177, 191)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(if app.status.is_empty() {
                "Waiting for setup…"
            } else {
                &app.status
            })
            .size(13)
            .width(Fill),
        ]
        .spacing(8),
    )
    .padding(15)
    .width(Fill)
    .style(container::bordered_box)
    .into()
}

fn path_text(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "Not selected".to_owned())
}

impl TrayHandle {
    fn new() -> Result<Self, String> {
        let open = MenuItem::with_id(TRAY_OPEN, "Open WoWs Replay Watcher", true, None);
        let quit = MenuItem::with_id(TRAY_QUIT, "Quit", true, None);
        let menu = Menu::with_items(&[&open, &quit]).map_err(|error| error.to_string())?;
        let icon =
            Icon::from_rgba(monitor_logo_rgba(64), 64, 64).map_err(|error| error.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("WoWs Replay Watcher")
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { _icon: icon })
    }
}

fn monitor_logo_rgba(size: u32) -> Vec<u8> {
    let logo = image::load_from_memory(include_bytes!("../assets/wows-monitor-icon.png"))
        .expect("embedded WoWs Monitor icon is a valid PNG")
        .to_rgba8();
    image::imageops::resize(&logo, size, size, image::imageops::FilterType::Lanczos3).into_raw()
}
