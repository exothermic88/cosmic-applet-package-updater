use cosmic::app::{Core, Task};
use cosmic::cosmic_config::Config;
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::window;
use cosmic::iced::{time, Subscription, window::Id, Limits};
use cosmic::widget::{
    autosize, button, divider, scrollable, space, text, text_input, toggler, Column, Row, Space,
};
use cosmic::Element;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::PackageUpdaterConfig;
use crate::updates::{self, SourceState, SystemTools, UpdateReport, UpdateTarget};

pub struct CosmicAppletPackageUpdater {
    core: Core,
    popup: Option<Id>,
    active_tab: PopupTab,
    config: PackageUpdaterConfig,
    config_handler: Config,
    report: UpdateReport,
    tools: SystemTools,
    last_check: Option<Instant>,
    checking_updates: bool,
    error_message: Option<String>,
    ignore_next_sync: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupTab {
    Updates,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    SwitchTab(PopupTab),
    CheckForUpdates,
    UpdatesChecked(SystemTools, UpdateReport),
    ConfigChanged(PackageUpdaterConfig),
    LaunchUpdate(UpdateTarget),
    TerminalFinished,
    Timer,
    SetCheckInterval(u32),
    ToggleAutoCheck(bool),
    ToggleCheckAur(bool),
    ToggleCheckFlatpak(bool),
    ToggleShowNotifications(bool),
    ToggleShowUpdateCount(bool),
    SetPreferredTerminal(String),
    SyncFileChanged,
}

impl cosmic::Application for CosmicAppletPackageUpdater {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.github.cosmic_ext.PackageUpdater";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let (config_handler, config) = PackageUpdaterConfig::load();
        let tools = SystemTools::detect();

        let app = Self {
            core,
            popup: None,
            active_tab: PopupTab::Updates,
            config,
            config_handler,
            report: UpdateReport::default(),
            tools,
            last_check: None,
            checking_updates: false,
            error_message: None,
            ignore_next_sync: true,
        };

        let mut tasks = vec![];

        // Check for updates on startup, with a delay to allow the system to stabilize
        if app.config.auto_check_on_startup {
            tasks.push(Task::perform(
                async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                },
                |_| cosmic::Action::App(Message::CheckForUpdates),
            ));
        }

        (app, Task::batch(tasks))
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        if self.config.show_update_count {
            // Always show custom button with icon and count (empty string when 0)
            let count_text = if self.report.total() > 0 {
                format!("{}", self.report.total())
            } else {
                String::new()
            };

            let custom_button = button::custom(
                Row::new()
                    .align_y(cosmic::iced::Alignment::Center)
                    .spacing(2)
                    .push(cosmic::widget::icon::from_name(self.get_icon_name()).size(16))
                    .push(text(count_text).size(12)),
            )
            .padding([8, 4])
            .class(cosmic::theme::Button::AppletIcon)
            .on_press(Message::TogglePopup);

            let limits = Limits::NONE.min_width(1.0).min_height(1.0);

            let content: Element<_> = if self.report.has_updates() {
                cosmic::widget::mouse_area(custom_button)
                    .on_middle_press(Message::LaunchUpdate(UpdateTarget::All))
                    .into()
            } else {
                custom_button.into()
            };

            autosize::autosize(content, cosmic::widget::Id::unique())
                .limits(limits)
                .into()
        } else {
            let icon_button = self
                .core
                .applet
                .icon_button(self.get_icon_name())
                .on_press(Message::TogglePopup);

            if self.report.has_updates() {
                cosmic::widget::mouse_area(icon_button)
                    .on_middle_press(Message::LaunchUpdate(UpdateTarget::All))
                    .into()
            } else {
                icon_button.into()
            }
        }
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let cosmic::cosmic_theme::Spacing {
            space_s, space_m, ..
        } = cosmic::theme::active().cosmic().spacing;

        // Tab bar
        let updates_button = button::text(if self.active_tab == PopupTab::Updates {
            "● Updates"
        } else {
            "○ Updates"
        })
        .on_press(Message::SwitchTab(PopupTab::Updates));

        let settings_button = button::text(if self.active_tab == PopupTab::Settings {
            "● Settings"
        } else {
            "○ Settings"
        })
        .on_press(Message::SwitchTab(PopupTab::Settings));

        let tabs = Row::new()
            .width(cosmic::iced::Length::Fill)
            .push(updates_button)
            .push(cosmic::widget::container(space::horizontal()).width(cosmic::iced::Length::Fill))
            .push(settings_button);

        // Tab content
        let tab_content = match self.active_tab {
            PopupTab::Updates => self.view_updates_tab(),
            PopupTab::Settings => self.view_settings_tab(),
        };

        // Package illustration - dynamic based on update status
        let (icon_name, emoji) = if self.checking_updates {
            ("view-refresh-symbolic", "⏳")
        } else if self.report.has_updates() {
            ("software-update-available-symbolic", "🎁")
        } else {
            ("package-x-generic", "✅")
        };

        let status_text = if self.checking_updates {
            text("Checking...")
                .size(11)
                .align_x(cosmic::iced::Alignment::Center)
        } else if self.report.has_updates() {
            text(format!("{} Updates", self.report.total()))
                .size(11)
                .align_x(cosmic::iced::Alignment::Center)
        } else {
            text("Up to Date")
                .size(11)
                .align_x(cosmic::iced::Alignment::Center)
        };

        let package_illustration = cosmic::widget::container(
            Column::new()
                .align_x(cosmic::iced::Alignment::Center)
                .spacing(12)
                .push(cosmic::widget::icon::from_name(icon_name).size(48))
                .push(text(emoji).size(28))
                .push(status_text),
        )
        .width(cosmic::iced::Length::Fixed(110.0))
        .height(cosmic::iced::Length::Fixed(150.0))
        .align_x(cosmic::iced::alignment::Horizontal::Center)
        .align_y(cosmic::iced::alignment::Vertical::Center)
        .style(|_theme| cosmic::widget::container::Style {
            background: None,
            ..Default::default()
        })
        .padding(12);

        // Main content area with illustration
        let main_content = Row::new()
            .spacing(space_m)
            .push(
                Column::new()
                    .spacing(space_s)
                    .width(cosmic::iced::Length::Fill)
                    .push(tab_content),
            )
            .push(package_illustration);

        let content = Column::new()
            .spacing(space_s)
            .padding(space_m)
            .push(tabs)
            .push(divider::horizontal::default())
            .push(main_content);

        self.core
            .applet
            .popup_container(content)
            .limits(
                Limits::NONE
                    .min_height(350.0)
                    .max_height(800.0)
                    .min_width(450.0)
                    .max_width(550.0),
            )
            .into()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::TogglePopup => self.handle_toggle_popup(),
            Message::PopupClosed(id) => self.handle_popup_closed(id),
            Message::SwitchTab(tab) => self.handle_switch_tab(tab),
            Message::CheckForUpdates => {
                if self.checking_updates {
                    return Task::none();
                }
                self.checking_updates = true;
                self.error_message = None;
                let check_aur = self.config.include_aur_updates;
                let check_flatpak = self.config.include_flatpak_updates;
                Task::perform(
                    async move {
                        // Re-detect tools on every check so newly installed
                        // helpers (paru/yay/flatpak) show up without a restart
                        let tools = SystemTools::detect();
                        let report = updates::check_all(tools, check_aur, check_flatpak).await;
                        (tools, report)
                    },
                    |(tools, report)| {
                        cosmic::Action::App(Message::UpdatesChecked(tools, report))
                    },
                )
            }
            Message::UpdatesChecked(tools, report) => {
                self.checking_updates = false;
                self.tools = tools;
                self.error_message = report
                    .all_failed()
                    .then(|| "Update checks failed. See sections for details.".to_string());
                self.report = report;
                self.last_check = Some(Instant::now());
                Task::none()
            }
            Message::LaunchUpdate(target) => {
                let Some(command) = target.command(&self.tools) else {
                    return Task::none();
                };
                let terminal = self.config.preferred_terminal.clone();
                Self::launch_terminal_task(terminal, command)
            }
            Message::TerminalFinished => {
                // Terminal has finished, trigger update check immediately
                Task::done(cosmic::Action::App(Message::CheckForUpdates))
            }
            Message::ConfigChanged(config) => {
                self.config = config;
                PackageUpdaterConfig::set_entry(&self.config_handler, &self.config);
                Task::none()
            }
            Message::Timer => {
                if self.checking_updates {
                    Task::none()
                } else {
                    Task::done(cosmic::Action::App(Message::CheckForUpdates))
                }
            }
            Message::SetCheckInterval(interval) => {
                let mut config = self.config.clone();
                config.check_interval_minutes = interval;
                Task::done(cosmic::Action::App(Message::ConfigChanged(config)))
            }
            Message::ToggleAutoCheck(enabled) => {
                let mut config = self.config.clone();
                config.auto_check_on_startup = enabled;
                Task::done(cosmic::Action::App(Message::ConfigChanged(config)))
            }
            Message::ToggleCheckAur(enabled) => {
                let mut config = self.config.clone();
                config.include_aur_updates = enabled;
                Task::done(cosmic::Action::App(Message::ConfigChanged(config)))
            }
            Message::ToggleCheckFlatpak(enabled) => {
                let mut config = self.config.clone();
                config.include_flatpak_updates = enabled;
                Task::done(cosmic::Action::App(Message::ConfigChanged(config)))
            }
            Message::ToggleShowNotifications(enabled) => {
                let mut config = self.config.clone();
                config.show_notifications = enabled;
                Task::done(cosmic::Action::App(Message::ConfigChanged(config)))
            }
            Message::ToggleShowUpdateCount(enabled) => {
                let mut config = self.config.clone();
                config.show_update_count = enabled;
                Task::done(cosmic::Action::App(Message::ConfigChanged(config)))
            }
            Message::SetPreferredTerminal(terminal) => {
                let mut config = self.config.clone();
                config.preferred_terminal = terminal;
                Task::done(cosmic::Action::App(Message::ConfigChanged(config)))
            }
            Message::SyncFileChanged => {
                // Ignore the first sync event on startup (file creation triggers watcher)
                if self.ignore_next_sync {
                    self.ignore_next_sync = false;
                    return Task::none();
                }

                // Another instance completed an update check, sync our state
                // Only sync if we're not already checking and haven't checked very recently
                if !self.checking_updates {
                    // Only sync if our last check was more than 10 seconds ago
                    let should_sync = self
                        .last_check
                        .is_none_or(|last| last.elapsed().as_secs() > 10);

                    if should_sync {
                        Task::done(cosmic::Action::App(Message::CheckForUpdates))
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
        }
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let timer_subscription =
            time::every(Duration::from_secs(
                self.config.check_interval_minutes as u64 * 60,
            ))
            .map(|_| Message::Timer);

        // File watcher subscription to sync with other instances
        let sync_subscription = Subscription::run(Self::watch_sync_file);

        Subscription::batch(vec![timer_subscription, sync_subscription])
    }
}

impl CosmicAppletPackageUpdater {
    fn get_sync_path() -> PathBuf {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(runtime_dir).join("cosmic-package-updater.sync")
    }

    fn watch_sync_file() -> impl futures::Stream<Item = Message> {
        use futures::channel::mpsc;
        use futures::StreamExt;
        use notify::{Event, RecursiveMode, Watcher};

        async_stream::stream! {
            let sync_path = Self::get_sync_path();

            // Ensure the parent directory exists
            if let Some(parent) = sync_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            // Create the sync file if it doesn't exist
            if !sync_path.exists() {
                let _ = std::fs::File::create(&sync_path);
            }

            let (tx, mut rx) = mpsc::unbounded();

            let mut watcher = match notify::recommended_watcher(move |res: Result<Event, _>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() || event.kind.is_create() {
                        let _ = tx.unbounded_send(());
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to create file watcher: {}", e);
                    return;
                }
            };

            if let Err(e) = watcher.watch(&sync_path, RecursiveMode::NonRecursive) {
                eprintln!("Failed to watch sync file: {}", e);
                return;
            }

            while rx.next().await.is_some() {
                // Small delay to avoid rapid fire events
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                yield Message::SyncFileChanged;
            }
        }
    }

    fn launch_terminal_task(terminal: String, command: String) -> Task<Message> {
        Task::perform(
            async move {
                // Create a unique marker file to track when the terminal closes
                let runtime_dir =
                    std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
                let marker_file = format!(
                    "{}/cosmic-package-updater-terminal-{}.marker",
                    runtime_dir,
                    std::process::id()
                );

                // Create the marker file
                let _ = std::fs::File::create(&marker_file);

                // Build command that removes marker file when done
                let wrapped_command = format!(
                    "{} && echo \"Update completed. Press Enter to exit...\" && read; rm -f \"{}\"",
                    command.replace("\"", "\\\""),
                    marker_file
                );

                // Spawn the terminal (it will return immediately due to daemonization)
                match tokio::process::Command::new(&terminal)
                    .arg("-e")
                    .arg("sh")
                    .arg("-c")
                    .arg(&wrapped_command)
                    .spawn()
                {
                    Ok(_) => {
                        // Poll for marker file deletion (terminal closed)
                        loop {
                            if !std::path::Path::new(&marker_file).exists() {
                                break;
                            }
                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        }

                        // Add a delay to allow system to stabilize after update
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    }
                    Err(_) => {
                        // Clean up marker file on error
                        let _ = std::fs::remove_file(&marker_file);
                    }
                }
            },
            |()| cosmic::Action::App(Message::TerminalFinished),
        )
    }

    fn handle_toggle_popup(&mut self) -> Task<Message> {
        if let Some(p) = self.popup.take() {
            destroy_popup(p)
        } else {
            // Add error handling for popup creation
            if let Some(main_window_id) = self.core.main_window_id() {
                let new_id = Id::unique();
                self.popup.replace(new_id);
                let mut popup_settings = self.core.applet.get_popup_settings(
                    main_window_id,
                    new_id,
                    None,
                    None,
                    None,
                );
                popup_settings.positioner.size_limits = Limits::NONE
                    .max_width(550.0)
                    .min_width(450.0)
                    .min_height(350.0)
                    .max_height(800.0);

                Task::batch(vec![get_popup(popup_settings), window::gain_focus(new_id)])
            } else {
                eprintln!("Failed to get main window ID for popup");
                self.error_message = Some("Unable to open popup window".to_string());
                Task::none()
            }
        }
    }

    fn handle_popup_closed(&mut self, id: Id) -> Task<Message> {
        if self.popup.as_ref() == Some(&id) {
            self.popup = None;
            self.active_tab = PopupTab::Updates;
        }
        Task::none()
    }

    fn handle_switch_tab(&mut self, tab: PopupTab) -> Task<Message> {
        self.active_tab = tab;
        Task::none()
    }

    fn get_icon_name(&self) -> &'static str {
        if self.checking_updates {
            "view-refresh-symbolic"
        } else if self.error_message.is_some() {
            "dialog-error-symbolic"
        } else if self.report.has_updates() {
            "software-update-available-symbolic"
        } else {
            "package-x-generic-symbolic"
        }
    }

    fn view_updates_tab(&self) -> Element<'_, Message> {
        let mut widgets: Vec<Element<'_, Message>> = vec![];

        // Status text
        if self.checking_updates {
            widgets.push(text("Checking for updates...").size(18).into());
        } else if let Some(error) = &self.error_message {
            widgets.push(text(format!("Error: {}", error)).size(18).into());
        } else if self.report.has_updates() {
            widgets.push(
                text(format!("{} updates available", self.report.total()))
                    .size(18)
                    .into(),
            );
        } else {
            widgets.push(text("System is up to date").size(18).into());
        }

        // Last check time
        if let Some(last_check) = self.last_check {
            let elapsed = last_check.elapsed();
            let time_text = if elapsed.as_secs() < 60 {
                "Last checked: just now".to_string()
            } else if elapsed.as_secs() < 3600 {
                format!("Last checked: {} minutes ago", elapsed.as_secs() / 60)
            } else {
                format!("Last checked: {} hours ago", elapsed.as_secs() / 3600)
            };
            widgets.push(text(time_text).size(12).into());
        }

        widgets.push(Space::new().height(cosmic::iced::Length::Fixed(16.0)).into());

        // Check button
        widgets.push(
            button::text("Check for Updates")
                .on_press(Message::CheckForUpdates)
                .width(cosmic::iced::Length::Fill)
                .into(),
        );

        // Update All button right after Check for Updates if updates available
        if self.report.has_updates() {
            widgets.push(
                button::text("Update All")
                    .on_press(Message::LaunchUpdate(UpdateTarget::All))
                    .width(cosmic::iced::Length::Fill)
                    .into(),
            );
            widgets.push(text("💡 Tip: Middle-click on the Panel icon").size(10).into());
            widgets.push(Space::new().height(cosmic::iced::Length::Fixed(8.0)).into());
        }

        // Per-source sections separated by dividers
        let aur_title = match self.tools.aur_helper {
            Some(helper) => format!("AUR ({})", helper.name()),
            None => "AUR".to_string(),
        };

        let sections: Vec<Element<'_, Message>> = [
            self.source_section("Pacman", &self.report.pacman, UpdateTarget::Pacman),
            self.source_section(&aur_title, &self.report.aur, UpdateTarget::Aur),
            self.source_section("Flatpak", &self.report.flatpak, UpdateTarget::Flatpak),
        ]
        .into_iter()
        .flatten()
        .collect();

        if !sections.is_empty() {
            let mut section_list = Column::new().spacing(8);
            let count = sections.len();
            for (i, section) in sections.into_iter().enumerate() {
                section_list = section_list.push(section);
                if i + 1 < count {
                    section_list = section_list.push(divider::horizontal::default());
                }
            }

            widgets.push(
                cosmic::widget::container(
                    scrollable(section_list)
                        .width(cosmic::iced::Length::Fill)
                        .height(cosmic::iced::Length::Fixed(320.0)),
                )
                .class(cosmic::theme::Container::List)
                .padding(12)
                .width(cosmic::iced::Length::Fill)
                .into(),
            );
        }

        Column::new().spacing(8).extend(widgets).into()
    }

    fn source_section<'a>(
        &'a self,
        title: &str,
        state: &'a SourceState,
        target: UpdateTarget,
    ) -> Option<Element<'a, Message>> {
        let content: Element<'a, Message> = match state {
            SourceState::Disabled => return None,
            SourceState::Error(error) => text(format!("Error: {}", error)).size(10).into(),
            SourceState::Checked(packages) if packages.is_empty() => {
                text("Up to date").size(10).into()
            }
            SourceState::Checked(packages) => {
                let mut list = Column::new().spacing(4);
                for package in packages {
                    let package_text = if package.current_version != "unknown" {
                        format!(
                            "  {} {} → {}",
                            package.name, package.current_version, package.new_version
                        )
                    } else {
                        format!("  {} → {}", package.name, package.new_version)
                    };
                    list = list.push(text(package_text).size(10));
                }
                list.into()
            }
        };

        let mut header = Row::new()
            .spacing(8)
            .align_y(cosmic::iced::Alignment::Center)
            .push(text(format!("{} ({})", title, state.count())).size(14))
            .push(Space::new().width(cosmic::iced::Length::Fill));

        if state.count() > 0 {
            header = header.push(
                button::text("Update")
                    .on_press(Message::LaunchUpdate(target)),
            );
        }

        Some(
            Column::new()
                .spacing(4)
                .push(header)
                .push(content)
                .into(),
        )
    }

    fn view_settings_tab(&self) -> Element<'_, Message> {
        let mut widgets: Vec<Element<'_, Message>> = vec![];

        // Check interval
        widgets.push(text("Check Interval (minutes)").size(14).into());
        let interval_value = self.config.check_interval_minutes.to_string();
        widgets.push(
            text_input("60", interval_value)
                .on_input(|s| Message::SetCheckInterval(s.parse::<u32>().unwrap_or(60).clamp(1, 1440)))
                .width(cosmic::iced::Length::Fill)
                .into(),
        );

        widgets.push(Space::new().height(cosmic::iced::Length::Fixed(8.0)).into());

        // Toggles
        widgets.push(
            Row::new()
                .spacing(8)
                .align_y(cosmic::iced::Alignment::Center)
                .push(text("Auto-check on startup"))
                .push(Space::new().width(cosmic::iced::Length::Fill))
                .push(toggler(self.config.auto_check_on_startup).on_toggle(Message::ToggleAutoCheck))
                .into(),
        );

        if self.tools.aur_helper.is_some() {
            widgets.push(
                Row::new()
                    .spacing(8)
                    .align_y(cosmic::iced::Alignment::Center)
                    .push(text("Include AUR updates"))
                    .push(Space::new().width(cosmic::iced::Length::Fill))
                    .push(toggler(self.config.include_aur_updates).on_toggle(Message::ToggleCheckAur))
                    .into(),
            );
        } else {
            widgets.push(text("Install paru or yay for AUR support").size(10).into());
        }

        if self.tools.flatpak {
            widgets.push(
                Row::new()
                    .spacing(8)
                    .align_y(cosmic::iced::Alignment::Center)
                    .push(text("Include Flatpak updates"))
                    .push(Space::new().width(cosmic::iced::Length::Fill))
                    .push(
                        toggler(self.config.include_flatpak_updates)
                            .on_toggle(Message::ToggleCheckFlatpak),
                    )
                    .into(),
            );
        }

        widgets.push(
            Row::new()
                .spacing(8)
                .align_y(cosmic::iced::Alignment::Center)
                .push(text("Show notifications"))
                .push(Space::new().width(cosmic::iced::Length::Fill))
                .push(toggler(self.config.show_notifications).on_toggle(Message::ToggleShowNotifications))
                .into(),
        );

        widgets.push(
            Row::new()
                .spacing(8)
                .align_y(cosmic::iced::Alignment::Center)
                .push(text("Show update count"))
                .push(Space::new().width(cosmic::iced::Length::Fill))
                .push(toggler(self.config.show_update_count).on_toggle(Message::ToggleShowUpdateCount))
                .into(),
        );

        widgets.push(Space::new().height(cosmic::iced::Length::Fixed(8.0)).into());

        // Terminal setting
        widgets.push(text("Preferred Terminal").size(14).into());
        let terminal_value = if self.config.preferred_terminal.is_empty() {
            "cosmic-term".to_string()
        } else {
            self.config.preferred_terminal.clone()
        };
        widgets.push(
            text_input("cosmic-term", terminal_value)
                .on_input(Message::SetPreferredTerminal)
                .width(cosmic::iced::Length::Fill)
                .into(),
        );

        Column::new().spacing(8).extend(widgets).into()
    }
}
