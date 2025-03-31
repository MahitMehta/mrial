use std::{process::Command, time::Duration};

use kanal::{AsyncSender, SendError};
use log::{debug, error, trace};
use tokio::task::JoinHandle;

use super::VideoServerAction;

#[derive(PartialEq, Clone, Copy)]
pub enum Setting {
    #[cfg(not(target_os = "linux"))]
    Unknown,
    PreLogin,
    PostLogin,
}

/*
 *  Configures the X environment for the server by setting
 *  correct display and Xauthority variables.
 *
 *  Additionally, it sets the XDG_RUNTIME_DIR and DBUS_SESSION_BUS_ADDRESS
 *  for pipewire connection from root.
 */

// TODO: Make DISPLAY variable dynamic AND
// TODO: don't assume the display manager is lightdm

#[cfg(target_os = "linux")]
pub fn config_xenv() -> Result<Setting, Box<dyn std::error::Error>> {
    use std::env;

    env::set_var("DISPLAY", ":0");

    if let Ok(Some(username)) = get_x11_authenicated_client() {
        /*
         * Environment variables needed to connect to
         * user graphical user session from root
         */
        let xauthority_path = format!("/home/{}/.Xauthority", username);
        debug!("Xauthority User Path: {}", xauthority_path);
        env::set_var("XAUTHORITY", xauthority_path);

        /*
         * Environment variables needed for pipewire connection from root.
         */
        let user_id_cmd = format!("id -u {}", username);
        let user_id_output = Command::new("sh").arg("-c").arg(user_id_cmd).output()?;

        let user_id = String::from_utf8(user_id_output.stdout)?;
        let xdg_runtime_dir = format!("/run/user/{}", user_id.trim());
        let dbus_session_bus_address = format!("unix:path={}/bus", xdg_runtime_dir);

        debug!("XDG_RUNTIME_DIR: {}", &xdg_runtime_dir);
        debug!("DBUS_SESSION_BUS_ADDRESS: {}", &dbus_session_bus_address);

        env::set_var("XDG_RUNTIME_DIR", xdg_runtime_dir);
        env::set_var("DBUS_SESSION_BUS_ADDRESS", dbus_session_bus_address);

        return Ok(Setting::PostLogin);
    }

    debug!("No user logged in to graphical session");
    env::set_var("XAUTHORITY", "/var/lib/lightdm/.Xauthority");
    return Ok(Setting::PreLogin);
}

#[cfg(target_os = "linux")]
fn get_x11_authenicated_client() -> Result<Option<String>, Box<dyn std::error::Error + Send>> {
    let gui_users_output = Command::new("sh")
        .arg("-c")
        .arg("who | grep tty7")
        .output()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

    if gui_users_output.stdout.is_empty() || !gui_users_output.status.success() {
        return Ok(None);
    }

    let output_str = String::from_utf8(gui_users_output.stdout)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

    if let Some(user) = output_str.split_whitespace().next() {
        return Ok(Some(user.to_string()));
    }

    Ok(None)
}

const SESSION_CHECK_INTERVAL: u64 = 1;

pub struct SessionSettingTask {
    setting: Setting,
    video_server_ch_sender: AsyncSender<VideoServerAction>,
}

impl SessionSettingTask {
    #[cfg(target_os = "linux")]
    async fn check_x11_user_logged_in(&mut self) -> Result<(), SendError> {
        match get_x11_authenicated_client() {
            Ok(Some(_)) => {
                debug!("User has logged in");

                self.setting = Setting::PostLogin;
                self.video_server_ch_sender
                    .send(VideoServerAction::NewUserSession)
                    .await?;
            }
            Err(e) => {
                error!("Error checking for X11 authenticated client: {:?}", e);
            }
            _ => {}
        }
        trace!("Waiting for user to login");

        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn check_x11_user_logged_out(&mut self) -> Result<(), SendError> {
        match get_x11_authenicated_client() {
            Ok(None) => {
                debug!("User has logged out");

                self.setting = Setting::PreLogin;
                self.video_server_ch_sender
                    .send(VideoServerAction::RestartSession)
                    .await?;
            }
            Err(e) => {
                error!("Error checking for X11 authenticated client: {:?}", e);
            }
            _ => {}
        }
        trace!("Waiting for user to logout");

        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn x11_session_status_loop(&mut self) -> Result<(), SendError> {
        use tokio::time::sleep;

        loop {
            match self.setting {
                Setting::PreLogin => {
                    self.check_x11_user_logged_in().await?;
                }
                Setting::PostLogin => {
                    self.check_x11_user_logged_out().await?;
                }
                #[cfg(not(target_os = "linux"))]
                Setting::Unknown => {}
            }

            sleep(Duration::from_secs(SESSION_CHECK_INTERVAL)).await;
        }
    }

    pub fn run(
        video_server_ch_sender: AsyncSender<VideoServerAction>,
        setting: Setting,
    ) -> JoinHandle<()> {
        let handle = tokio::runtime::Handle::current();

        return handle.spawn(async move {
            let mut session_setting_thread = SessionSettingTask {
                video_server_ch_sender,
                setting,
            };

            #[cfg(target_os = "linux")]
            if let Err(e) = session_setting_thread.x11_session_status_loop().await {
                error!("X11 session status loop crashed: {:?}", e);
            }
        });
    }
}
