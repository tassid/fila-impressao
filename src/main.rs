#![windows_subsystem = "windows"]

mod app;
mod force;
mod spooler;
mod status;

fn main() -> eframe::Result {
    // Relaunched elevated by the "Forçar" button: no window, just do the job.
    if std::env::args().any(|a| a == force::FORCE_ARG) {
        std::process::exit(force::elevated_main());
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([480.0, 380.0])
            .with_min_inner_size([400.0, 260.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Fila de Impressão",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
