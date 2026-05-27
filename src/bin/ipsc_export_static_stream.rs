use direct_stream_game::{
    CustomHostBranding, CustomHostLayout, static_palette_stream_page_html_with_options,
};
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), String> {
    let mut backend = "https://game.humanbeangames.com".to_owned();
    let mut branding = CustomHostBranding::default();
    let mut layout = CustomHostLayout::default();
    let mut args = env::args().skip(1).peekable();
    if let Some(first) = args.peek() {
        if !first.starts_with("--") {
            backend = args.next().unwrap_or(backend);
        }
    }
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--page-title" => {
                branding.page_title = args
                    .next()
                    .ok_or_else(|| "--page-title requires a value".to_owned())?;
            }
            "--header-title" => {
                branding.header_title = args
                    .next()
                    .ok_or_else(|| "--header-title requires a value".to_owned())?;
            }
            "--max-player-width" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--max-player-width requires a pixel value".to_owned())?;
                layout.max_player_width_px = Some(
                    value
                        .parse()
                        .map_err(|_| format!("invalid --max-player-width value: {value}"))?,
                );
            }
            "--prefer-larger-player" => layout.prefer_larger_player = true,
            "--minimizable-player" => layout.minimizable_player = true,
            "--start-player-minimized" => {
                layout.minimizable_player = true;
                layout.start_player_minimized = true;
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            _ => return Err(format!("unknown option: {arg}")),
        }
    }

    let out_dir = PathBuf::from("dist/humanbeangames_stream");
    fs::create_dir_all(&out_dir).map_err(|err| err.to_string())?;
    fs::write(
        out_dir.join("index.html"),
        static_palette_stream_page_html_with_options(&backend, &branding, &layout),
    )
    .map_err(|err| err.to_string())?;

    println!(
        "Exported stream player to {} using backend {}",
        out_dir.display(),
        backend
    );
    Ok(())
}

fn print_help() {
    println!(
        "\
Usage:
  cargo run --bin ipsc_export_static_stream -- [backend-origin] [options]

Options:
  --page-title <text>          Browser page title.
  --header-title <text>        Visible page header text.
  --max-player-width <px>      Maximum stream player width in CSS pixels.
  --prefer-larger-player       Use a larger default player cap.
  --minimizable-player         Show a minimize/restore stream button.
  --start-player-minimized     Start minimized and enable the button.
"
    );
}
