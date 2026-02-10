use slint::{ComponentHandle, Weak};
use crate::{AppWindow, MessageBox};
use crate::settings::ThreadSafeSettings;
use crate::sound_thread::spawn_sound_thread;

pub async fn spawn_message_box(message: String, app_window: Weak<AppWindow>, settings: ThreadSafeSettings) {
    let app = app_window.upgrade();
    if let Some(_app) = app {
        let message_box = MessageBox::new().expect("Failed to create message box");
        message_box.set_message(message.into());

        message_box.on_message_box_closed({
            let message_box_handle = message_box.clone_strong();
            move || {
                message_box_handle.window().hide().unwrap();
            }
        });
        message_box.show().unwrap();
        let locked_settings = settings.lock().await;
        if locked_settings.play_sound {
            spawn_sound_thread(locked_settings.sound_source.clone());
        }
    }
}