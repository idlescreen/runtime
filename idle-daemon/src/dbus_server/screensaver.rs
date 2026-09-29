// SPDX-License-Identifier: MIT

use crate::controller::{DaemonCommand, DaemonController};
use std::sync::Arc;

pub struct ScreenSaverService {
    pub controller: Arc<DaemonController>,
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaverService {
    pub(crate) async fn inhibit(
        &self,
        application_name: &str,
        reason_for_inhibit: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<u32> {
        let sender = header.sender().ok_or_else(|| {
            zbus::fdo::Error::Failed("inhibit request missing D-Bus sender".into())
        })?;
        idle_log::info!(
            "ScreenSaver: Inhibit requested by {} ({}): {}",
            sender,
            application_name,
            reason_for_inhibit
        );
        let cookie = self
            .controller
            .inhibitors
            .add(
                application_name.to_string(),
                reason_for_inhibit.to_string(),
                sender.to_owned(),
            )
            .map_err(|error| zbus::fdo::Error::LimitsExceeded(error.to_string()))?;
        let _ = self
            .controller
            .send_command(DaemonCommand::StopPresentation);
        self.controller.mark_dirty();
        Ok(cookie)
    }

    pub(crate) async fn un_inhibit(
        &self,
        cookie: u32,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = header.sender().ok_or_else(|| {
            zbus::fdo::Error::Failed("un_inhibit request missing D-Bus sender".into())
        })?;
        idle_log::info!(
            "ScreenSaver: UnInhibit requested by {} for cookie {}",
            sender,
            cookie
        );
        if !self.controller.inhibitors.remove_for_client(cookie, sender) {
            return Err(zbus::fdo::Error::Failed(format!(
                "unknown inhibit cookie for caller: {cookie}"
            )));
        }
        self.controller.mark_dirty();
        Ok(())
    }

    pub(crate) async fn simulate_user_activity(&self) {
        idle_log::info!("ScreenSaver: SimulateUserActivity requested");
        let _ = self
            .controller
            .send_command(DaemonCommand::StopPresentation);
    }

    pub(crate) async fn get_active(&self) -> bool {
        let active = self
            .controller
            .status
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .presentation_active;
        idle_log::debug!("ScreenSaver: GetActive requested: {}", active);
        active
    }

    pub(crate) async fn set_active(
        &self,
        active: bool,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        idle_log::info!("ScreenSaver: SetActive requested: {}", active);
        if active {
            super::service_helpers::authorize_control(&self.controller, &header).await?;
            let config = self
                .controller
                .config
                .lock()
                .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
                .clone();
            let saver = crate::daemon::presentation::pick_saver_name(
                &config,
                crate::daemon::presentation::current_time_micros(),
            );
            self.controller
                .send_command(DaemonCommand::Preview(saver))
                .map_err(|_| zbus::fdo::Error::LimitsExceeded("Command queue full".into()))?;
        } else {
            self.controller
                .send_command(DaemonCommand::StopPresentation)
                .map_err(|_| zbus::fdo::Error::LimitsExceeded("Command queue full".into()))?;
        }
        self.controller.mark_dirty();
        Ok(())
    }

    pub(crate) async fn lock(&self) {
        idle_log::info!("ScreenSaver: Lock requested");
        let _ = self
            .controller
            .send_command(DaemonCommand::StopPresentation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DaemonConfig;
    use crate::controller::DaemonCommand;

    #[tokio::test]
    async fn test_simulate_user_activity() {
        let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
        let service = ScreenSaverService {
            controller: controller.clone(),
        };

        service.simulate_user_activity().await;

        let commands = controller.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], DaemonCommand::StopPresentation));
    }

    #[tokio::test]
    async fn test_get_active() {
        let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
        let service = ScreenSaverService {
            controller: controller.clone(),
        };

        assert!(!service.get_active().await);

        controller
            .status
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p))
            .presentation_active = true;
        assert!(service.get_active().await);
    }

    #[tokio::test]
    async fn test_set_active() {
        let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
        let service = ScreenSaverService {
            controller: controller.clone(),
        };

        let msg = zbus::message::Message::method_call("/org/freedesktop/ScreenSaver", "SetActive")
            .unwrap()
            .build(&(true,))
            .unwrap();
        let header = msg.header();

        // active = true should fail with D-Bus connection unavailable error
        assert!(service.set_active(true, header.clone()).await.is_err());
        let commands = controller.drain_commands();
        assert_eq!(commands.len(), 0);

        // active = false does not require authorization and should succeed
        assert!(service.set_active(false, header.clone()).await.is_ok());
        let commands = controller.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], DaemonCommand::StopPresentation));
    }

    #[tokio::test]
    async fn test_lock() {
        let controller = Arc::new(DaemonController::new(DaemonConfig::default()));
        let service = ScreenSaverService {
            controller: controller.clone(),
        };

        service.lock().await;
        let commands = controller.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0], DaemonCommand::StopPresentation));
    }
}
