mod audio;
mod controller;
mod logging;
mod models;
mod paths;
mod state_machine;
mod storage;

pub fn run() {
    controller::run_app();
}
