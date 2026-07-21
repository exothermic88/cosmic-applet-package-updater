mod app;
mod config;
mod updates;

use app::CosmicAppletPackageUpdater;

fn main() -> cosmic::iced::Result {
    cosmic::applet::run::<CosmicAppletPackageUpdater>(())
}