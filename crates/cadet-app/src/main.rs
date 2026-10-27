#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod commands;
mod devtools;
mod features;
mod files;
mod icons;
mod kernel;
mod measure;
mod python;
mod settings;
#[cfg(target_os = "macos")]
mod macos;
mod sketch;
mod snap;
mod theme;

fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--kernel") {
        cadet_core::worker::serve();
        cadet_core::kernel::exit_now(0);
    }
    let viewport = egui::ViewportBuilder::default()
        .with_title("Cadet")
        .with_inner_size([1400.0, 880.0])
        .with_min_inner_size([720.0, 480.0])
        .with_fullsize_content_view(true)
        .with_titlebar_shown(!cfg!(target_os = "macos"))
        .with_title_shown(!cfg!(target_os = "macos"))
        .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png")).unwrap_or_default());
    #[cfg(target_os = "macos")]
    macos::install();
    let scripted = std::env::var_os("CADET_SCRIPT").is_some() || std::env::var_os("CADET_SCREENSHOT").is_some();
    let viewport = if scripted { viewport.with_position([0.0, 0.0]).with_active(false) } else { viewport };
    let options = eframe::NativeOptions {
        viewport,
        #[cfg(target_os = "macos")]
        event_loop_builder: Some(Box::new(|b| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            b.with_default_menu(false);
        })),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native("Cadet", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
