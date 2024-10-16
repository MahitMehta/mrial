use log::debug;
use mrial_fs::{storage::StorageSingletonType, AppState, Server, User};
use slint::{ComponentHandle, SharedString, VecModel};
use std::rc::Rc;

use super::super::slint_generatedMainWindow::{
    HomeAdapter, HostingAdapter, IServer, IUser, MainWindow, StartAdapter,
};

pub fn populate_users(users: Vec<User>, app_weak: &slint::Weak<MainWindow>) {
    let slint_users = Rc::new(VecModel::default());
    for user in users {
        slint_users.push(IUser {
            username: SharedString::from(user.username),
            enabled: true,
        });
    }
    app_weak
        .unwrap()
        .global::<HostingAdapter>()
        .set_users(slint_users.into());
}

pub fn populate_servers(servers: Vec<Server>, app_weak: &slint::Weak<MainWindow>) {
    let slint_servers = Rc::new(VecModel::default());
    for server in servers {
        slint_servers.push(IServer {
            name: SharedString::from(server.name),
            address: SharedString::from(server.address),
            port: server.port.into(),
            os: SharedString::from(server.os),
            ram: 24,
            storage: 40,
            vcpu: 4,
        });
    }
    app_weak
        .unwrap()
        .global::<HomeAdapter>()
        .set_servers(slint_servers.into());
}

pub fn handle_app_state(app_state: AppState, app_weak: &slint::Weak<MainWindow>) {
    let app_state_ref = app_state.get();
    let state = match app_state_ref.lock() {
        Ok(state) => state,
        Err(_) => return,
    };

    let state = match state.as_ref() {
        Some(state) => state,
        None => return,
    };

    if state.passed_setup {
        app_weak.unwrap().set_page(1);
        return;
    }

    let app_weak_clone = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        app_weak_clone
            .unwrap()
            .global::<StartAdapter>()
            .on_passed_setup(move || {
                let app_state_ref = app_state.get();
                let mut state = match app_state_ref.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };

                if let Some(state) = state.as_mut() {
                    state.passed_setup = true;
                }

                drop(state);
                if let Err(e) = app_state.save() {
                    debug!("Failed to Save App State: {}", e);
                }
            });
    });
}
