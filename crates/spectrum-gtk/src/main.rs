use std::path::Path;

use glib as glib;
use gtk4 as gtk;
use gtk4::prelude::*;
use spectrum_gtk::app::App;

fn main() {
    let folder = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    gtk::init().expect("gtk init failed");

    let app = App::new(Path::new(&folder));
    app.window.present();

    let main_loop = glib::MainLoop::new(None, true);
    main_loop.run();
}
