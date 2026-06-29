use tray_icon::{
    menu::{Menu, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

pub fn init_tray() -> Result<TrayIcon, Box<dyn std::error::Error>> {
    let tray_menu = Menu::new();
    let show_item = MenuItem::with_id("show_app", "Show App", true, None);
    let quit_item = MenuItem::with_id("quit_app", "Quit App", true, None);

    tray_menu.append(&show_item)?;
    tray_menu.append(&quit_item)?;

    let icon = load_default_icon();

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Cache Advisor")
        .with_icon(icon)
        .build()?;

    Ok(tray)
}

fn load_default_icon() -> Icon {
    let width = 32;
    let height = 32;
    let mut rgba = vec![0u8; width * height * 4];

    // Create a beautiful green checkmark/cleaning brush color pattern
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            // Draw a circle with color gradient
            let dist_from_center = (((x as f32 - 16.0).powi(2) + (y as f32 - 16.0).powi(2)).sqrt()) / 16.0;
            if dist_from_center <= 1.0 {
                rgba[idx] = 46;      // R
                rgba[idx + 1] = 204; // G
                rgba[idx + 2] = 113; // B
                rgba[idx + 3] = (255.0 * (1.0 - dist_from_center.powi(2))) as u8; // Alpha fading
            } else {
                rgba[idx] = 0;
                rgba[idx + 1] = 0;
                rgba[idx + 2] = 0;
                rgba[idx + 3] = 0; // Transparent
            }
        }
    }

    Icon::from_rgba(rgba, width as u32, height as u32).unwrap()
}
