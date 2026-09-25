use tauri::{ActivationPolicy, App};

pub fn configure(app: &mut App) {
    app.set_activation_policy(ActivationPolicy::Accessory);
}
