mod ai;
mod asr;
mod audio;
mod controller;
mod importer;
mod logging;
mod models;
mod paths;
mod process_icons;
mod state_machine;
mod storage;
mod voiceprints;

pub fn run() {
    controller::run_app();
}
