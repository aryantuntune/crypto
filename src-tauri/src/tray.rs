use tauri::{
    tray::TrayIconBuilder,
    menu::{Menu, MenuItem},
    AppHandle, Manager, PhysicalPosition, PhysicalSize,
};

pub fn init_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle_item = MenuItem::with_id(app, "toggle", "Toggle Panel", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle_item, &quit_item])?;

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .icon(app.default_window_icon().unwrap().clone())
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "toggle" => toggle_panel(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn toggle_panel(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let visible = win.is_visible().unwrap_or(false);
        if visible {
            let _ = win.hide();
        } else {
            position_panel(&win);
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

fn position_panel(win: &tauri::WebviewWindow) {
    if let Ok(monitors) = win.available_monitors() {
        let monitor = win
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| monitors.into_iter().next());
        if let Some(m) = monitor {
            let size = m.size();
            let pos = m.position();
            let panel_w = 420u32;
            let panel_h = 800u32;
            let scale = m.scale_factor();
            let x = pos.x + (size.width as f64 / scale) as i32 - panel_w as i32 - 16;
            let y = pos.y + 60;
            let _ = win.set_size(PhysicalSize::new(
                (panel_w as f64 * scale) as u32,
                (panel_h as f64 * scale) as u32,
            ));
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
    }
}
