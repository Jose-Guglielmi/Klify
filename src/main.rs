//! Entry point del binario. Toda la app vive en la librería (`src/lib.rs`)
//! para que los tests de integración puedan usarla.

fn main() {
    // Icono de la app (ventana / barra de tareas). Se embebe en el binario.
    let icon: dioxus::desktop::tao::window::Icon =
        dioxus::desktop::icon_from_memory(include_bytes!("../assets/icon.png"))
            .expect("no se pudo cargar assets/icon.png");

    // Ventana con tamaño MÍNIMO = exactamente lo que el usuario eligió:
    // 1233 × 628 de CONTENIDO (el exterior con barra de título es 1233×656).
    // Por debajo, el layout del modo VS se rompe. Aplica en macOS y Windows.
    let window = dioxus::desktop::WindowBuilder::new()
        .with_title("Klify")
        .with_window_icon(Some(icon))
        .with_min_inner_size(dioxus::desktop::LogicalSize::new(1233.0, 628.0));
    let cfg = dioxus::desktop::Config::new().with_window(window);

    dioxus::LaunchBuilder::new().with_cfg(cfg).launch(klipy::App);
}
